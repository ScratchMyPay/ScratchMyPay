"""Fake rgb-lib backend implementing Bill's RgbBackend protocol; counts real sends."""

from __future__ import annotations

import hashlib
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

import pytest

HERE = Path(__file__).resolve()
sys.path.insert(0, str(HERE.parents[1]))  # services/wallet-owner

from poweredge_wallet.bill import load_bill  # noqa: E402
from poweredge_wallet.journal import Journal  # noqa: E402
from poweredge_wallet.service import (  # noqa: E402
    AssetRow, ServiceConfig, SimulatedCrash, TransferRow, WalletService,
)

BILL = load_bill()
backend_mod = sys.modules["web.rgb.backend"]
ASSET_ID = "rgb:I7olLl3y-c49Ha7C-v82SFEj-BU33dvn-4mGtCIL-MNRyWm8"
INVOICE = "rgb:I7olLl3y-c49Ha7C-v82SFEj-BU33dvn-4mGtCIL-MNRyWm8/~/wvout:fake-recipient-1"
REVISION = "a" * 40
TEMPLATE_DIGEST = "fe668c53ce71f152a83e370c9c4d038c9c26d465edaa24af307888952b362917"


@dataclass
class FakeBackend:
    tmp: Path
    balance: int = 1_000_000_000_000
    sends: list = field(default_factory=list)
    transfers: list = field(default_factory=list)
    fail_send_after_broadcast: bool = False
    imported: list = field(default_factory=list)

    def spendable_balance(self, asset_id: str) -> int:
        return self.balance

    def witness_receive(self, asset_id, expiration_timestamp, min_confirmations):
        return backend_mod.ReceiveIntent(INVOICE, "fake-recipient-1", expiration_timestamp, 7)

    def parse_invoice(self, invoice: str):
        return backend_mod.ParsedInvoice(
            invoice=invoice, recipient_id="fake-recipient-1", asset_id=ASSET_ID,
            expiration_timestamp=int(time.time()) + 3600, transport_endpoints=(),
            unknown_query_params={})

    def send_donation(self, *, asset_id, recipient_id, amount, **_):
        self.sends.append((asset_id, recipient_id, amount))
        path = self.tmp / f"send-{len(self.sends)}.rgbc"
        path.write_bytes(b"consignment-bytes-" + str(len(self.sends)).encode())
        txid = hashlib.sha256(path.read_bytes()).hexdigest()
        self.transfers.append(TransferRow(len(self.sends), "WAITING_CONFIRMATIONS", "Send",
                                          txid, recipient_id, str(path)))
        if self.fail_send_after_broadcast:
            raise ConnectionError("indexer connection reset after broadcast")
        return backend_mod.BackendSendResult(txid, len(self.sends), path)

    def import_consignment(self, consignment_path: Path):
        self.imported.append(consignment_path.read_bytes())
        return {7: "WAITING_CONFIRMATIONS"}

    def refresh(self):
        return {}

    def close(self):
        pass


class FakeOps:
    def __init__(self, backend: FakeBackend) -> None:
        self.backend = backend

    def assets(self):
        return [AssetRow(ASSET_ID, "TUSDT", "PowerEdge Test USD", 6, self.backend.balance)]

    def transfers(self, asset_id):
        return list(self.backend.transfers)


def settings(tmp: Path, role: str):
    env = {
        "BILL_ROLE": "A" if role == "payer" else "B",
        "RGB_DATA_DIR_A": str(tmp / "a"), "RGB_DATA_DIR_B": str(tmp / "b"),
        "RGB_OOB_CONSIGNMENT": "1", f"RGB_OOB_INBOX_{'A' if role == 'payer' else 'B'}": str(tmp / "inbox"),
    }
    return BILL["config"].RgbSettings.from_env(env)


@dataclass
class Harness:
    tmp: Path
    backend: FakeBackend
    journal_path: Path
    role: str
    deliver: object = None
    crash_at: str | None = None

    def service(self) -> WalletService:
        adapter = BILL["adapter"].RgbBillAdapter(settings(self.tmp, self.role), self.backend)
        self.adapter = adapter

        def crash(point):
            raise SimulatedCrash(point)

        config = ServiceConfig(wallet_id=self.role, role=self.role, deliver=self.deliver,
                               crash_at=self.crash_at, crash=crash, sleep=lambda _: None)
        return WalletService(adapter, FakeOps(self.backend), Journal(self.journal_path), config,
                             errors=BILL["adapter"])


@pytest.fixture
def payer(tmp_path) -> Harness:
    return Harness(tmp_path, FakeBackend(tmp_path), tmp_path / "payer.sqlite3", "payer")


@pytest.fixture
def recipient(tmp_path) -> Harness:
    return Harness(tmp_path, FakeBackend(tmp_path), tmp_path / "recipient.sqlite3", "recipient")


def plan_request(**overrides):
    body = {
        "invoice": INVOICE, "asset_ticker": "tUSDT", "amount_base_units": 25_000_000,
        "amount_display": "25", "recipient_label": "sound engineer", "memo": "Saturday's show",
        "flow_revision": REVISION, "template_digest": TEMPLATE_DIGEST,
    }
    body.update(overrides)
    return body
