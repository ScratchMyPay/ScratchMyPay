"""Wiring to EdwinKestler/bill@be3d237 (prior work): settings, rgb-lib backend, adapter.

Bill is imported from BILL_ROOT (default: ../bill next to this repo) and used unchanged.
This module only adds the read-side calls the adapter does not expose.
"""

from __future__ import annotations

import importlib
import os
import sys
from pathlib import Path
from types import ModuleType
from typing import Any

from .service import AssetRow, TransferRow

REPO_ROOT = Path(__file__).resolve().parents[3]


def load_bill() -> dict[str, ModuleType]:
    root = Path(os.environ.get("BILL_ROOT", REPO_ROOT.parent / "bill")).resolve()
    if not (root / "web" / "rgb" / "adapter.py").is_file():
        raise RuntimeError(f"BILL_ROOT={root} does not contain Bill's web/rgb adapter")
    if str(root) not in sys.path:
        sys.path.insert(0, str(root))
    return {
        "adapter": importlib.import_module("web.rgb.adapter"),
        "config": importlib.import_module("web.rgb.config"),
        "native": importlib.import_module("web.rgb.native"),
    }


def _enum_name(value: Any) -> str:
    name = getattr(value, "name", None)
    return str(name if name is not None else value).split(".")[-1]


class RgbLibOps:
    """Read-side calls on the same open wallet the adapter owns (no second owner)."""

    def __init__(self, backend: Any) -> None:
        self._backend = backend

    def assets(self) -> list[AssetRow]:
        wallet = self._backend._wallet
        listed = wallet.list_assets([])
        rows = []
        for field in ("nia", "cfa", "ifa", "uda"):
            for asset in getattr(listed, field, None) or []:
                balance = wallet.get_asset_balance(asset.asset_id)
                rows.append(AssetRow(
                    asset_id=asset.asset_id,
                    ticker=getattr(asset, "ticker", None) or "",
                    name=asset.name,
                    precision=int(asset.precision),
                    spendable=int(balance.spendable),
                ))
        return rows

    def funds(self) -> dict[str, Any]:
        """BTC balance + every UTXO with its RGB allocations. One sync, then read-only."""
        wallet, online = self._backend._wallet, self._backend._online
        bal = wallet.get_btc_balance(online, False)
        unspents = wallet.list_unspents(online, False, True)

        def b(x: Any) -> dict[str, int]:
            return {"settled": int(x.settled), "future": int(x.future), "spendable": int(x.spendable)}

        return {
            "btc": {"vanilla": b(bal.vanilla), "colored": b(bal.colored)},
            "utxos": [
                {
                    "outpoint": f"{u.utxo.outpoint.txid}:{u.utxo.outpoint.vout}",
                    "sats": int(u.utxo.btc_amount),
                    "colorable": bool(u.utxo.colorable),
                    "exists": bool(u.utxo.exists),
                    "pending_blinded": int(u.pending_blinded),
                    "allocations": [
                        {"asset_id": a.asset_id, "settled": bool(a.settled),
                         "amount": int(getattr(a.assignment, "amount", 0) or 0)}
                        for a in u.rgb_allocations
                    ],
                }
                for u in unspents
            ],
        }

    def send_btc(self, address: str, amount_sat: int, fee_rate: int) -> str:
        """Plain-sats send (no RGB). rgb-lib takes the sats from uncoloured UTXOs only."""
        wallet, online = self._backend._wallet, self._backend._online
        return str(wallet.send_btc(online, address, int(amount_sat), int(fee_rate), False))

    def proxy_invoice(self, asset_id: str | None, expiration_timestamp: int, endpoint: str) -> dict[str, Any]:
        """Witness invoice that names an RGB proxy, so any RGB wallet or faucet can pay it."""
        wallet, rgb = self._backend._wallet, self._backend._rgb
        data = wallet.witness_receive(asset_id, rgb.Assignment.ANY(), int(expiration_timestamp), [endpoint], 1)
        return {"invoice": data.invoice, "recipient_id": data.recipient_id,
                "expiration_timestamp": int(data.expiration_timestamp),
                "batch_transfer_idx": int(data.batch_transfer_idx)}

    def transfers(self, asset_id: str | None) -> list[TransferRow]:
        return [
            TransferRow(
                batch_transfer_idx=int(t.batch_transfer_idx),
                status=_enum_name(t.status),
                kind=_enum_name(t.kind),
                txid=t.txid,
                recipient_id=t.recipient_id,
                consignment_path=t.consignment_path,
            )
            for t in self._backend._wallet.list_transfers(asset_id)
        ]


def open_wallet() -> tuple[Any, Any, RgbLibOps, ModuleType, Any]:
    """Open the env-configured wallet online. Returns (settings, adapter, ops, errors, backend)."""
    bill = load_bill()
    settings = bill["config"].RgbSettings.from_env(require_credentials=True)
    backend = bill["native"].RgbLibBackend.open(settings)
    adapter = bill["adapter"].RgbBillAdapter(settings, backend)
    return settings, adapter, RgbLibOps(backend), bill["adapter"], backend
