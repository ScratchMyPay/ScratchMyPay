#!/usr/bin/env python3
"""Issue the PowerEdge demo NIA asset (default tUSDT) on the payer wallet. Mutinynet only.

Reuses Bill's `RgbSettings` / `RgbLibBackend` (EdwinKestler/bill@be3d237, prior work) via
BILL_ROOT on sys.path. Requires a typed confirmation, never prints secrets, and writes a
public receipt (contract ID, supply, precision) to artifacts/asset/<ticker>.json.

Run from the repo root with the payer env loaded:
  set -a; . ./.env.payer; set +a
  BILL_ROOT=../bill ../bill/.venv/bin/python services/wallet-owner/scripts/issue_asset.py
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
LOG = logging.getLogger("poweredge.issue_asset")


def _load_bill():
    bill_root = Path(os.environ.get("BILL_ROOT", ROOT.parent / "bill")).resolve()
    sys.path.insert(0, str(bill_root))
    from web.rgb.config import BillRole, RgbSettings  # noqa: PLC0415
    from web.rgb.native import RgbLibBackend  # noqa: PLC0415

    return BillRole, RgbSettings, RgbLibBackend


def _validate(ticker: str, name: str, precision: int, supply_units: int) -> int:
    if not (1 <= len(ticker) <= 8 and ticker.isalnum()):
        raise ValueError("ticker must be 1-8 alphanumeric characters")
    if not 1 <= len(name) <= 64:
        raise ValueError("name must be 1-64 characters")
    if not 0 <= precision <= 18:
        raise ValueError("precision must be 0-18")
    if supply_units < 1:
        raise ValueError("supply must be positive")
    base = supply_units * 10**precision
    if base >= 2**64:
        raise ValueError("supply in base units exceeds u64")
    return base


def main() -> int:
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s: %(message)s")
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--ticker", default="tUSDT")
    parser.add_argument("--name", default="PowerEdge Test USD")
    parser.add_argument("--precision", type=int, default=6)
    parser.add_argument("--supply", type=int, default=1_000_000, help="whole units")
    args = parser.parse_args()

    if os.environ.get("POWEREDGE_NETWORK") != "mutinynet":
        LOG.error("POWEREDGE_NETWORK must be 'mutinynet'; refusing")
        return 2
    base_supply = _validate(args.ticker, args.name, args.precision, args.supply)
    BillRole, RgbSettings, RgbLibBackend = _load_bill()
    settings = RgbSettings.from_env(require_credentials=True)
    if settings.role is not BillRole.A:
        LOG.error("issuance is only allowed on the payer wallet (Bill role A)")
        return 2

    plan_id = str(uuid.uuid4())
    print(json.dumps({
        "confirmation_required": True, "plan_id": plan_id, "operation": "issue-asset-nia",
        "ticker": args.ticker, "name": args.name, "precision": args.precision,
        "supply_units": args.supply, "supply_base_units": base_supply,
        "network": "Mutinynet (custom signet)",
    }, indent=2), flush=True)
    expected = f"CONFIRM {plan_id}"
    if input(f"Type exactly '{expected}' to issue: ") != expected:
        LOG.error("confirmation did not match; nothing issued")
        return 1

    backend = RgbLibBackend.open(settings)
    try:
        asset = backend._wallet.issue_asset_nia(
            backend._online, args.ticker, args.name, args.precision, [base_supply]
        )
    finally:
        backend.close()

    receipt = {
        "asset_id": asset.asset_id,
        "schema": "NIA",
        "ticker": asset.ticker,
        "name": asset.name,
        "precision": asset.precision,
        "issued_supply_base_units": int(asset.issued_supply),
        "network": "mutinynet",
        "issued_at": datetime.now(timezone.utc).isoformat(),
        "plan_id": plan_id,
    }
    out_dir = ROOT / "artifacts" / "asset"
    out_dir.mkdir(parents=True, exist_ok=True)
    out = out_dir / f"{args.ticker}.json"
    out.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    LOG.info("issued %s: %s -> %s", args.ticker, asset.asset_id, out)
    print(json.dumps(receipt, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
