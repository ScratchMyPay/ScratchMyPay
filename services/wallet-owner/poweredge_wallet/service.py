"""Wallet-owner service logic: Bill's adapter (prior work) behind a durable journal.

Order of operations for a payout (payer):
  plan      -> journal PLANNED  (immutable plan + plan_digest bound to flow revision)
  approve   -> journal APPROVED (digest + flow revision must match; adapter.confirm_transfer)
  execute   -> journal SUBMITTING (durable) -> adapter send -> BROADCAST | UNKNOWN_SUBMISSION
  deliver   -> recipient imports the consignment -> DELIVERED
  status    -> wallet refresh -> SETTLED
Executing a plan that may already have been sent returns the journal row; it never sends.
"""

from __future__ import annotations

import base64
import hashlib
import json
import logging
import os
import random
import threading
import time
import uuid
from dataclasses import dataclass, field
from typing import Any, Callable, Protocol

from . import proxy as proxy_mod
from .journal import SENT_OR_MAYBE_SENT, Journal, JournalError, Plan, State

LOG = logging.getLogger("poweredge.wallet")

ENVIRONMENT = "mutinynet"
MAX_LABEL_CHARS = 64
MAX_MEMO_CHARS = 120


class ServiceError(RuntimeError):
    """Rejected request. `code` is stable and mapped to plain language by the guided app."""

    def __init__(self, code: str, message: str, status: int = 400) -> None:
        super().__init__(message)
        self.code = code
        self.status = status


class SimulatedCrash(BaseException):
    """Raised by the test crash hook in place of os._exit."""


@dataclass(frozen=True)
class AssetRow:
    asset_id: str
    ticker: str
    name: str
    precision: int
    spendable: int


@dataclass(frozen=True)
class TransferRow:
    batch_transfer_idx: int
    status: str
    kind: str
    txid: str | None
    recipient_id: str | None
    consignment_path: str | None


class WalletOps(Protocol):
    """Read-side wallet calls the adapter does not expose."""

    def assets(self) -> list[AssetRow]: ...

    def transfers(self, asset_id: str) -> list[TransferRow]: ...

    def funds(self) -> dict[str, Any]: ...

    def send_btc(self, address: str, amount_sat: int, fee_rate: int) -> str: ...

    def proxy_invoice(self, asset_id: str | None, expiration_timestamp: int, endpoint: str) -> dict[str, Any]: ...


@dataclass
class ServiceConfig:
    wallet_id: str
    role: str  # "payer" | "recipient"
    deliver: Callable[[bytes], dict[str, Any]] | None = None
    crash_at: str | None = None
    crash: Callable[[str], None] | None = None
    sleep: Callable[[float], None] = time.sleep
    clock: Callable[[], float] = time.time
    extra: dict[str, Any] = field(default_factory=dict)
    # RGB proxy delivery (when the crew's invoice names an allowed proxy)
    indexer_url: str = ""
    proxy_client: Any = None            # proxy.ProxyClient
    tx_outputs: Callable[[str], list[tuple[int, str]]] | None = None
    allowed_proxies: tuple[str, ...] = ()
    default_transport: str = "direct"   # crew invoices: direct | proxy


def display_matches(display: str, base_units: int, precision: int) -> bool:
    """True if a plain decimal display amount equals base_units at this precision, exactly."""
    from decimal import Decimal, InvalidOperation
    text = str(display).strip()
    if not text or not all(c.isdigit() or c == "." for c in text) or text.count(".") > 1:
        return False
    try:
        return Decimal(text) * (Decimal(10) ** precision) == Decimal(base_units)
    except InvalidOperation:
        return False


def canonical_digest(value: dict[str, Any]) -> str:
    return hashlib.sha256(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def with_backoff(fn: Callable[[], Any], *, sleep: Callable[[float], None], attempts: int = 5) -> Any:
    """Retry only on indexer rate limiting (HTTP 429). Everything else propagates."""
    for attempt in range(attempts):
        try:
            return fn()
        except Exception as exc:  # noqa: BLE001 - classified below
            if "429" not in str(exc) or attempt == attempts - 1:
                raise
            delay = min(30.0, 2.0 ** (attempt + 2)) + random.uniform(0, 1.5)
            LOG.warning("indexer rate-limited (429); retrying in %.1fs", delay)
            sleep(delay)
    raise AssertionError("unreachable")


class WalletService:
    def __init__(self, adapter: Any, ops: WalletOps, journal: Journal, config: ServiceConfig,
                 *, errors: Any) -> None:
        self._adapter = adapter
        self._ops = ops
        self._journal = journal
        self._cfg = config
        self._errors = errors  # Bill's adapter module: AdapterError, ConfirmationRequired, ...
        self._lock = threading.RLock()  # one wallet, one operation at a time
        self.recovered = journal.recover()

    # ---- read side -------------------------------------------------------------------
    def capabilities(self) -> dict[str, Any]:
        payer = self._cfg.role == "payer"
        return {
            "wallet_id": self._cfg.wallet_id,
            "role": self._cfg.role,
            "environment": ENVIRONMENT,
            "operations": (
                ["assets", "plan", "approve", "execute", "reconcile", "status", "refresh"]
                if payer else ["assets", "invoice", "import_consignment", "refresh"]
            ),
            "transport": "out-of-band consignment (no proxy)",
            "mainnet": False,
            "recovered_unknown_submissions": self.recovered,
        }

    def assets(self) -> list[dict[str, Any]]:
        with self._lock:
            return [vars(a) for a in self._ops.assets()]

    def funds(self, *, fee_rate: int = 2) -> dict[str, Any]:
        """Read-only: BTC balances, UTXO inventory and how many payouts the colored sats cover.

        A live PowerEdge payout is one tx of 197 vB with a 1,000 sat witness output, paid
        from the colored side (both live payouts had a single colored input).
        """
        if not 1 <= fee_rate <= 1000:
            raise ServiceError("invalid_request", "fee_rate must be 1..1000 sat/vB")
        with self._lock:
            raw = with_backoff(self._ops.funds, sleep=self._cfg.sleep)
        utxos = [u for u in raw["utxos"] if u["exists"]]
        colorable = [u for u in utxos if u["colorable"]]
        holding = [u for u in colorable if u["allocations"]]
        free = [u for u in colorable if not u["allocations"] and not u["pending_blinded"]]
        per_payout = 197 * fee_rate + 1000
        colored_spendable = raw["btc"]["colored"]["spendable"]
        return {
            "wallet_id": self._cfg.wallet_id,
            "role": self._cfg.role,
            "btc": raw["btc"],
            "utxos": {
                "total": len(utxos),
                "colorable": len(colorable),
                "holding_assets": len(holding),
                "free_colorable": len(free),
                "free_colorable_sats": sorted((u["sats"] for u in free), reverse=True),
                "holding": [{"outpoint": u["outpoint"], "sats": u["sats"],
                             "allocations": u["allocations"]} for u in holding],
            },
            "estimate": {
                "fee_rate_sat_vb": fee_rate,
                "per_payout_sat": per_payout,
                "payouts_from_colored_sats": colored_spendable // per_payout,
            },
        }

    # ---- small plain-sats payments (door passes) -------------------------------------
    SMALL_SATS_MAX = 5_000
    SMALL_SATS_MIN_INTERVAL = 30.0

    def pay_small_sats(self, *, address: str, amount_sat: int, fee_rate: int, purpose: str) -> dict[str, Any]:
        """Pay a few test sats (a PowerEdge door pass). Not an RGB transfer and not a payout:
        capped at SMALL_SATS_MAX, rate-limited, Mutinynet addresses only, journaled as an event."""
        self._require("payer")
        address = address.strip()
        if not (address.startswith("tb1") and 20 <= len(address) <= 90 and address.isalnum()):
            raise ServiceError("invalid_address", "only a Mutinynet (tb1…) address is accepted")
        if not 1 <= amount_sat <= self.SMALL_SATS_MAX:
            raise ServiceError("amount_too_large", f"small payments are capped at {self.SMALL_SATS_MAX} sats")
        if not 1 <= fee_rate <= 50:
            raise ServiceError("invalid_request", "fee_rate must be 1..50 sat/vB")
        with self._lock:
            last = getattr(self, "_last_small_sats", 0.0)
            now = self._cfg.clock()
            if now - last < self.SMALL_SATS_MIN_INTERVAL:
                raise ServiceError("rate_limited", "one small payment every 30 s", 429)
            self._last_small_sats = now
            txid = with_backoff(lambda: self._ops.send_btc(address, amount_sat, fee_rate), sleep=self._cfg.sleep)
            self._journal.note(f"sats:{txid}", f"paid {amount_sat} sats to {address} ({purpose[:60]})")
        LOG.info("small sats payment: %s sats to %s txid %s (%s)", amount_sat, address, txid, purpose)
        return {"txid": txid, "address": address, "amount_sat": amount_sat, "fee_rate": fee_rate, "purpose": purpose}

    def _asset_by_ticker(self, ticker: str) -> AssetRow:
        wanted = ticker.strip().upper()
        matches = [a for a in self._ops.assets() if a.ticker.upper() == wanted]
        if len(matches) != 1:
            raise ServiceError("unknown_asset", f"wallet holds no single asset with ticker {wanted}")
        return matches[0]

    # ---- recipient ---------------------------------------------------------------------
    def create_invoice(self, *, asset_id: str | None, expires_seconds: int, transport: str | None = None) -> dict[str, Any]:
        """`asset_id=None` invoices for any asset (needed before the first receipt).

        transport="direct": our organizer service hands the consignment over (crew only).
        transport="proxy": the invoice names the RGB proxy, so any RGB wallet or faucet can pay it.
        The organizer may create proxy invoices too: that is how it is funded (e.g. from a faucet).
        """
        transport = transport or (self._cfg.default_transport if self._cfg.role == "recipient" else "direct")
        if transport not in ("direct", "proxy"):
            raise ServiceError("invalid_request", "transport must be direct or proxy")
        if transport == "direct":
            self._require("recipient")
        if asset_id is not None and not asset_id.startswith("rgb:"):
            raise ServiceError("invalid_asset", "asset_id must be an RGB contract id (rgb:...)")
        if transport == "proxy":
            if not self._cfg.allowed_proxies:
                raise ServiceError("proxy_disabled", "no RGB proxy is configured on this service", 409)
            endpoint = self._cfg.allowed_proxies[0]
            expiry = int(self._cfg.clock()) + expires_seconds
            with self._lock:
                data = self._call(lambda: with_backoff(lambda: self._ops.proxy_invoice(asset_id, expiry, endpoint),
                                                       sleep=self._cfg.sleep))
            return {**data, "asset_id": asset_id, "environment": ENVIRONMENT, "transport": "proxy", "proxy": endpoint}
        with self._lock:
            intent = self._call(lambda: self._adapter.create_receive_intent(
                asset_id=asset_id, expires_in_seconds=expires_seconds))
        return {
            "invoice": intent.invoice,
            "recipient_id": intent.recipient_id,
            "expiration_timestamp": intent.expiration_timestamp,
            "batch_transfer_idx": intent.batch_transfer_idx,
            "asset_id": asset_id,
            "environment": ENVIRONMENT,
            "transport": "direct",
        }

    def import_consignment(self, payload: bytes) -> dict[str, Any]:
        self._require("recipient")
        sha = hashlib.sha256(payload).hexdigest()
        with self._lock:
            statuses = self._call(lambda: with_backoff(
                lambda: self._adapter.import_consignment(payload), sleep=self._cfg.sleep))
        statuses = {int(k): str(v) for k, v in dict(statuses).items()}
        self._journal.record_receipt(sha, len(payload), statuses)
        LOG.info("imported consignment %s (%d bytes): %s", sha, len(payload), statuses)
        return {"consignment_sha256": sha, "size": len(payload), "statuses": statuses}

    # ---- payer -------------------------------------------------------------------------
    def plan(self, req: dict[str, Any]) -> dict[str, Any]:
        self._require("payer")
        label = str(req.get("recipient_label", "")).strip()
        memo = str(req.get("memo", "")).strip()
        if not label or len(label) > MAX_LABEL_CHARS or len(memo) > MAX_MEMO_CHARS:
            raise ServiceError("invalid_plan", "recipient label or memo has an invalid length")
        amount = req.get("amount_base_units")
        if not isinstance(amount, int) or isinstance(amount, bool) or amount <= 0:
            raise ServiceError("invalid_amount", "amount_base_units must be a positive integer")
        for key in ("invoice", "flow_revision", "template_digest", "amount_display"):
            if not isinstance(req.get(key), str) or not req[key]:
                raise ServiceError("invalid_plan", f"{key} is required")
        with self._lock:
            asset = self._asset_by_ticker(str(req.get("asset_ticker", "")))
            # What the person sees must be exactly what is sent (an edited flow could differ).
            if not display_matches(req["amount_display"], amount, asset.precision):
                raise ServiceError("amount_mismatch",
                                   f"{req['amount_display']} {asset.ticker} is not {amount} base units "
                                   f"at {asset.precision} decimals; prepare the payout again")
            if amount > asset.spendable:
                raise ServiceError("insufficient_balance",
                                   f"wallet has {asset.spendable} base units of {asset.ticker}")
            parsed, plan = self._call(lambda: self._plan_from_invoice(
                req["invoice"].strip(), asset_id=asset.asset_id, amount=amount))
            fields = {
                "plan_id": plan.plan_id,
                "wallet_id": self._cfg.wallet_id,
                "environment": ENVIRONMENT,
                "asset_id": asset.asset_id,
                "asset_ticker": asset.ticker,
                "recipient_id": plan.recipient_id,
                "invoice": parsed.invoice,
                "amount_base_units": plan.amount,
                "amount_display": req["amount_display"],
                "recipient_label": label,
                "memo": memo,
                "flow_revision": req["flow_revision"],
                "template_digest": req["template_digest"],
                "expiration_timestamp": plan.expiration_timestamp,
            }
            fields["plan_digest"] = canonical_digest(fields)
            row = self._journal.insert_plan(fields)
        return self._view(row)

    def approve(self, plan_id: str, *, plan_digest: str, flow_revision: str, actor: str) -> dict[str, Any]:
        self._require("payer")
        if not actor.strip():
            raise ServiceError("invalid_approval", "approval needs the approver's name")
        with self._lock:
            row = self._get(plan_id)
            if row.state_enum is not State.PLANNED:
                raise ServiceError("not_approvable", f"plan is {row.state}, not awaiting approval", 409)
            if self._expired(row):
                self._journal.transition(plan_id, State.PLANNED, State.EXPIRED, detail="expired before approval")
                raise ServiceError("plan_expired", "the plan expired; prepare a new one", 409)
            if plan_digest != row.plan_digest:
                raise ServiceError("digest_mismatch", "the approved plan does not match the prepared plan", 409)
            if flow_revision != row.flow_revision:
                raise ServiceError("flow_changed", "the flow changed after this plan was prepared", 409)
            self._call(lambda: self._adapter.confirm_transfer(plan_id))
            row = self._journal.transition(
                plan_id, State.PLANNED, State.APPROVED, detail=f"approved by {actor}",
                approved_by=actor.strip()[:64], approved_at=self._cfg.clock())
        return self._view(row)

    def execute(self, plan_id: str, *, flow_revision: str, idempotency_key: str) -> dict[str, Any]:
        """Send exactly once. Re-execution returns the journal row and never sends again."""
        self._require("payer")
        if not idempotency_key or len(idempotency_key) > 128:
            raise ServiceError("invalid_idempotency_key", "an idempotency key is required")
        with self._lock:
            row = self._get(plan_id)
            if row.state_enum in SENT_OR_MAYBE_SENT:
                LOG.info("execute(%s) ignored: already %s", plan_id, row.state)
                return {**self._view(row), "already_executed": True}
            if row.state_enum is not State.APPROVED:
                raise ServiceError("not_approved", f"plan is {row.state}; an approval is required", 409)
            if flow_revision != row.flow_revision:
                self._journal.transition(plan_id, State.APPROVED, State.FAILED,
                                         detail="flow revision changed after approval",
                                         error="flow_changed")
                raise ServiceError("flow_changed", "the flow changed after approval; approval voided", 409)
            if self._expired(row):
                self._journal.transition(plan_id, State.APPROVED, State.EXPIRED, detail="expired before send")
                raise ServiceError("plan_expired", "the approved plan expired; prepare a new one", 409)
            # Durable BEFORE the irreversible step.
            self._journal.transition(plan_id, State.APPROVED, State.SUBMITTING,
                                     detail="about to broadcast", idempotency_key=idempotency_key)
            self._maybe_crash("before_send")
            try:
                artifact = self._adapter.execute_confirmed_transfer(plan_id)
            except self._errors.ConfirmationRequired as exc:
                # Raised by the adapter before any broadcast (consumed, expired, balance drift).
                row = self._journal.transition(plan_id, State.SUBMITTING, State.FAILED,
                                               detail="refused before broadcast", error=str(exc))
                raise ServiceError("not_sent", f"nothing was sent: {exc}", 409) from exc
            except Exception as exc:  # noqa: BLE001 - outcome unknown by definition
                self._journal.transition(plan_id, State.SUBMITTING, State.UNKNOWN_SUBMISSION,
                                         detail="send raised; outcome unknown", error=str(exc)[:500])
                raise ServiceError("unknown_submission",
                                   "the send may or may not have happened; reconcile before anything else",
                                   502) from exc
            # Worst moment to die: money has left, the journal still says SUBMITTING.
            self._maybe_crash("after_broadcast")
            row = self._journal.transition(
                plan_id, State.SUBMITTING, State.BROADCAST, detail=f"txid {artifact.txid}",
                txid=artifact.txid, batch_transfer_idx=artifact.batch_transfer_idx,
                consignment_sha256=artifact.sha256, consignment_path=str(artifact.path),
                consignment_size=artifact.size)
            self._maybe_crash("after_send")
        return self._deliver(row)

    def deliver(self, plan_id: str) -> dict[str, Any]:
        """Re-deliver the consignment to the recipient (idempotent on the recipient side)."""
        self._require("payer")
        return self._deliver(self._get(plan_id))

    def reconcile(self, plan_id: str) -> dict[str, Any]:
        """Resolve UNKNOWN_SUBMISSION from wallet evidence. Never sends."""
        self._require("payer")
        with self._lock:
            row = self._get(plan_id)
            if row.state_enum is not State.UNKNOWN_SUBMISSION:
                return self._view(row)
            self._call(lambda: with_backoff(self._adapter.refresh_transfers, sleep=self._cfg.sleep))
            sends = [t for t in self._ops.transfers(row.asset_id)
                     if t.kind.lower() == "send" and t.recipient_id == row.recipient_id and t.txid]
            if len(sends) == 1:
                sent = sends[0]
                payload = self._read_consignment(sent.consignment_path)
                row = self._journal.transition(
                    plan_id, State.UNKNOWN_SUBMISSION, State.BROADCAST,
                    detail=f"reconciled from wallet: txid {sent.txid}", txid=sent.txid,
                    batch_transfer_idx=sent.batch_transfer_idx,
                    consignment_path=sent.consignment_path,
                    consignment_sha256=hashlib.sha256(payload).hexdigest() if payload else None,
                    consignment_size=len(payload) if payload else None)
            elif not sends:
                row = self._journal.transition(
                    plan_id, State.UNKNOWN_SUBMISSION, State.FAILED,
                    detail="reconciled: wallet has no send to this recipient; nothing was broadcast",
                    error="no_submission_evidence")
            else:
                raise ServiceError("ambiguous_reconcile", "more than one send matches; manual review", 409)
        return self._view(row)

    def status(self, plan_id: str, *, refresh: bool) -> dict[str, Any]:
        with self._lock:
            row = self._get(plan_id)
            delivering = self._delivers(row)
            if refresh and delivering and row.state_enum is State.BROADCAST:
                # Delivery failed earlier (e.g. indexer race): retry. Never re-sends.
                self._deliver(row)
                row = self._get(plan_id)
            # "Settled" means the recipient holds it: with a peer, only after it imported.
            settle_from = State.DELIVERED if delivering else State.BROADCAST
            if refresh and row.state_enum is settle_from:
                self._call(lambda: with_backoff(self._adapter.refresh_transfers, sleep=self._cfg.sleep))
                endpoint = self._proxy_endpoint(row)
                for transfer in self._ops.transfers(row.asset_id):
                    if transfer.batch_transfer_idx == row.batch_transfer_idx and transfer.status == "SETTLED":
                        if endpoint is None:
                            row = self._journal.transition(plan_id, row.state_enum, State.SETTLED,
                                                           detail="payer transfer SETTLED; recipient imported")
                        else:
                            ack = self._proxy_ack(row, endpoint)
                            if ack is True:
                                row = self._journal.transition(
                                    plan_id, row.state_enum, State.SETTLED,
                                    detail="payer transfer SETTLED; recipient ACKed via RGB proxy")
                            elif ack is False:
                                LOG.warning("plan %s: recipient NACKed the consignment on the proxy", plan_id)
                        break
        return {**self._view(row), "events": self._journal.events(plan_id)}

    def has_pending_incoming(self) -> bool:
        """True while any incoming transfer waits for the counterparty or for confirmations."""
        with self._lock:
            return any(
                "RECEIVE" in t.kind.upper() and t.status.upper().startswith("WAITING")
                for asset in self._ops.assets()
                for t in self._ops.transfers(asset.asset_id)
            )

    def auto_refresh_tick(self) -> dict[str, Any] | None:
        """One background step: refresh only while something incoming is still pending.

        rgb-lib marks a received transfer settled only when the wallet refreshes after the
        witness tx confirms; without this the crew wallet stays "pending" indefinitely.
        """
        if not self.has_pending_incoming():
            return None
        return self.refresh()

    def refresh(self) -> dict[str, Any]:
        with self._lock:
            updated = self._call(lambda: with_backoff(self._adapter.refresh_transfers, sleep=self._cfg.sleep))
        return {"updated_transfers": {int(k): str(v) for k, v in dict(updated).items()}}

    # ---- internals ---------------------------------------------------------------------
    def _plan_from_invoice(self, invoice: str, *, asset_id: str, amount: int) -> tuple[Any, Any]:
        """Bill's tap-invoice rules, plus: a first-time recipient may invoice for *any* asset.

        rgb-lib refuses to name an asset the recipient wallet has never held, so a crew
        member's first invoice carries no asset id. The payer still pins its own contract.
        """
        errors = self._errors
        parsed = self._adapter._backend.parse_invoice(invoice)
        if parsed.invoice != invoice:
            raise errors.AdapterError("RGB invoice did not round-trip canonically")
        if parsed.transport_endpoints:
            try:
                endpoint = proxy_mod.proxy_for(invoice, self._cfg.allowed_proxies)
            except proxy_mod.ProxyError as exc:
                raise errors.AdapterError(str(exc)) from exc
            if endpoint is None:
                raise errors.AdapterError("invoice transport endpoints do not match the allowed RGB proxy")
            if "wvout:" not in parsed.recipient_id:
                raise errors.AdapterError("proxy delivery is supported for witness invoices only")
        if parsed.unknown_query_params:
            raise errors.AdapterError("invoice contains unsupported query parameters")
        if parsed.asset_id is not None and parsed.asset_id != asset_id:
            raise errors.AdapterError("invoice asks for a different asset")
        now = int(self._cfg.clock())
        if parsed.expiration_timestamp is None or parsed.expiration_timestamp <= now:
            raise errors.AdapterError("invoice is expired or has no expiration")
        lifetime = min(900, parsed.expiration_timestamp - now)
        if lifetime < 60:
            raise errors.AdapterError("invoice expires too soon for a reviewed payout")
        plan = self._adapter.plan_transfer(asset_id=asset_id, recipient_id=parsed.recipient_id,
                                           amount=amount, expires_in_seconds=lifetime)
        return parsed, plan

    def _proxy_endpoint(self, row: Plan) -> str | None:
        try:
            return proxy_mod.proxy_for(row.invoice, self._cfg.allowed_proxies)
        except proxy_mod.ProxyError:
            return None

    def _delivers(self, row: Plan) -> bool:
        return self._proxy_endpoint(row) is not None or self._cfg.deliver is not None

    def _proxy_ack(self, row: Plan, endpoint: str) -> bool | None:
        try:
            return self._cfg.proxy_client.get_ack(endpoint, recipient_id=row.recipient_id)
        except Exception as exc:  # noqa: BLE001 - checked again on the next refresh
            LOG.warning("ack check for %s failed: %s", row.plan_id, exc)
            return None

    def _deliver_via_proxy(self, row: Plan, endpoint: str) -> dict[str, Any]:
        payload = self._read_consignment(row.consignment_path)
        if not payload or hashlib.sha256(payload).hexdigest() != row.consignment_sha256:
            raise ServiceError("consignment_missing", "consignment file is missing or changed", 500)
        try:
            outputs = self._cfg.tx_outputs(row.txid)
            vout = proxy_mod.find_vout(outputs, proxy_mod.witness_script(row.recipient_id))
            self._cfg.proxy_client.post_consignment(endpoint, recipient_id=row.recipient_id, txid=row.txid,
                                                    vout=vout, payload=payload)
        except Exception as exc:  # noqa: BLE001 - delivery can be retried safely; nothing is re-sent
            LOG.warning("proxy delivery of %s failed: %s", row.plan_id, exc)
            return {**self._view(row), "delivery_error": str(exc)[:300]}
        row = self._journal.transition(row.plan_id, State.BROADCAST, State.DELIVERED,
                                       detail=f"consignment posted to RGB proxy {endpoint} (vout {vout})")
        return {**self._view(row), "delivered_via": endpoint}

    def _deliver(self, row: Plan) -> dict[str, Any]:
        if row.state_enum is not State.BROADCAST:
            return self._view(row)
        endpoint = self._proxy_endpoint(row)
        if endpoint is not None:
            return self._deliver_via_proxy(row, endpoint)
        if self._cfg.deliver is None:
            return self._view(row)
        payload = self._read_consignment(row.consignment_path)
        if not payload or hashlib.sha256(payload).hexdigest() != row.consignment_sha256:
            raise ServiceError("consignment_missing", "consignment file is missing or changed", 500)
        try:
            receipt = self._cfg.deliver(payload)
        except Exception as exc:  # noqa: BLE001 - delivery can be retried safely
            LOG.warning("delivery of %s failed: %s", row.plan_id, exc)
            return {**self._view(row), "delivery_error": str(exc)[:300]}
        row = self._journal.transition(row.plan_id, State.BROADCAST, State.DELIVERED,
                                       detail=f"recipient imported: {receipt.get('statuses')}")
        return {**self._view(row), "recipient_receipt": receipt}

    @staticmethod
    def _read_consignment(path: str | None) -> bytes | None:
        if not path:
            return None
        try:
            with open(path, "rb") as handle:
                return handle.read()
        except OSError:
            return None

    def _maybe_crash(self, point: str) -> None:
        if self._cfg.crash_at != point:
            return
        LOG.error("CRASH INJECTION at %s (POWEREDGE_CRASH_AT)", point)
        if self._cfg.crash is not None:
            self._cfg.crash(point)
        else:
            os._exit(137)

    def _expired(self, row: Plan) -> bool:
        return int(row.expiration_timestamp) <= int(self._cfg.clock())

    def _get(self, plan_id: str) -> Plan:
        try:
            uuid.UUID(plan_id)
            return self._journal.get(plan_id)
        except (ValueError, JournalError) as exc:
            raise ServiceError("unknown_plan", "no such payout plan", 404) from exc

    def _require(self, role: str) -> None:
        if self._cfg.role != role:
            raise ServiceError("wrong_wallet", f"this operation needs the {role} wallet", 403)

    def _call(self, fn: Callable[[], Any]) -> Any:
        try:
            return fn()
        except ServiceError:
            raise
        except self._errors.AdapterError as exc:
            raise ServiceError("wallet_refused", str(exc), 409) from exc
        except Exception as exc:  # noqa: BLE001 - rgb-lib errors become a stable code, not a 500
            LOG.exception("wallet operation failed")
            raise ServiceError("wallet_error", f"{type(exc).__name__}: {exc}"[:300], 502) from exc

    @staticmethod
    def _view(row: Plan) -> dict[str, Any]:
        public = dict(row.row)
        public.pop("consignment_path", None)  # local path stays local
        return public


def encode_consignment(payload: bytes) -> str:
    return base64.b64encode(payload).decode("ascii")


def decode_consignment(text: str, limit: int) -> bytes:
    try:
        payload = base64.b64decode(text, validate=True)
    except ValueError as exc:
        raise ServiceError("invalid_consignment", "consignment must be base64") from exc
    if not payload or len(payload) > limit:
        raise ServiceError("invalid_consignment", "consignment is empty or too large")
    return payload
