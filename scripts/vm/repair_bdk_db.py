#!/usr/bin/env python3
"""Repair a torn bdk_db (BDK's append-only bitcoin wallet store) after a power-cut style stop.

Works on a SCRATCH copy of a wallet data dir, never the live one. Opens the wallet OFFLINE through Bill
(load_offline: no network, cannot send), trying truncation lengths from the end of the file backwards,
and keeps the longest prefix that opens. The dropped tail is the torn last entry: cached chain data that
the next online sync fetches again. RGB state (rgb/, rgb_lib_db, transfers/) is not touched.

  services/wallet-owner/.venv/bin/python scripts/vm/repair_bdk_db.py --role payer \
      --scratch .vm/repair/payer --min-size 23472
"""
from __future__ import annotations

import argparse
import os
import shutil
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def load_env(path: Path) -> dict[str, str]:
    env = {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            env[k] = v
    return env


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--role", choices=["payer", "recipient"], required=True)
    p.add_argument("--scratch", required=True, help="scratch copy of datadirs/<role> (contains <fingerprint>/bdk_db)")
    p.add_argument("--min-size", type=int, default=0, help="known-good prefix length (e.g. the pre-VM file size)")
    a = p.parse_args()
    letter = "A" if a.role == "payer" else "B"
    scratch = Path(a.scratch).resolve()
    if str(scratch).startswith(str((ROOT / "datadirs").resolve())):
        sys.exit("refusing: --scratch must not be under datadirs/ (never repair the live copy in place)")
    dbs = list(scratch.glob("*/bdk_db"))
    if len(dbs) != 1:
        sys.exit(f"expected one */bdk_db under {scratch}, found {len(dbs)}")
    db = dbs[0]
    original = db.read_bytes()
    shutil.copy2(db, db.with_name("bdk_db.torn"))

    env = load_env(ROOT / f".env.{a.role}")
    env[f"RGB_DATA_DIR_{letter}"] = str(scratch)
    env[f"RGB_OOB_INBOX_{letter}"] = str(scratch.parent / f"oob-{a.role}")
    os.environ.update(env)
    sys.path.insert(0, os.environ.get("BILL_ROOT", str(ROOT.parent / "bill")))
    from web.rgb.config import RgbSettings  # noqa: E402  (Bill, unchanged)
    from web.rgb.native import RgbLibBackend  # noqa: E402

    settings = RgbSettings.from_env(require_credentials=True)
    t0 = time.time()
    tried = 0
    for size in range(len(original), a.min_size - 1, -1):
        db.write_bytes(original[:size])
        tried += 1
        try:
            backend = RgbLibBackend.load_offline(settings)
        except Exception as exc:  # noqa: BLE001 - any open failure means "not a valid prefix"
            if tried == 1:
                print(f"full file does not open: {str(exc)[:120]}")
            continue
        assets = backend._wallet.list_assets([])
        n = len(assets.nia or []) + len(getattr(assets, "cfa", None) or [])
        print(f"OPENS at {size} bytes (dropped {len(original) - size} torn bytes) after {tried} tries, "
              f"{time.time() - t0:.1f}s; assets known offline: {n}")
        return 0
    db.write_bytes(original)
    print(f"no valid prefix between {a.min_size} and {len(original)} bytes")
    return 1


if __name__ == "__main__":
    sys.exit(main())
