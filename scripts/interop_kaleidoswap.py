#!/usr/bin/env python3
"""Interop test: can a ScratchMyPay-style wallet receive RGB assets from KaleidoSwap's tools?

Uses a FRESH throwaway rgb-lib wallet (never the live organizer/crew wallets: one wallet must
never be open in two places). Same settings as our wallets (rgb-lib 0.3.0b7, SIGNET_CUSTOM =
Mutinynet, Taproot, Esplora https://mutinynet.com/api), plus what our wallets lack today:
invoices that carry an RGB proxy endpoint, so any RGB wallet or faucet can deliver the
consignment.

  scripts/interop_kaleidoswap.py init                       create the test wallet (once)
  scripts/interop_kaleidoswap.py invoice [--asset ID]       witness invoice with the proxy endpoint
  scripts/interop_kaleidoswap.py status                     refresh from the proxy + indexer, show balances

Seed: secrets/interop.mnemonic (0600, git-ignored). Data: datadirs/interop/ (git-ignored).
Evidence of each step: artifacts/interop/kaleidoswap-<date>.json.
"""
from __future__ import annotations

import argparse
import json
import logging
import os
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import rgb_lib

ROOT = Path(__file__).resolve().parents[1]
SEED = ROOT / "secrets" / "interop.mnemonic"
DATA = ROOT / "datadirs" / "interop"
META = DATA / "wallet.json"
INDEXER = os.environ.get("INTEROP_INDEXER", "https://mutinynet.com/api")
PROXY = os.environ.get("INTEROP_PROXY", "rpcs://proxy.kaleidoswap.com/json-rpc")
EVIDENCE = ROOT / "artifacts" / "interop" / f"kaleidoswap-{datetime.now().date().isoformat()}.json"
LOG = logging.getLogger("interop")


def record(step: str, data: dict) -> None:
    EVIDENCE.parent.mkdir(parents=True, exist_ok=True)
    log = json.loads(EVIDENCE.read_text()) if EVIDENCE.exists() else {"what": "ScratchMyPay x KaleidoSwap RGB interop test", "proxy": PROXY, "indexer": INDEXER, "steps": []}
    log["steps"].append({"at": datetime.now(timezone.utc).isoformat(timespec="seconds"), "step": step, **data})
    EVIDENCE.write_text(json.dumps(log, indent=1) + "\n")


def wallet_data() -> rgb_lib.WalletData:
    return rgb_lib.WalletData(
        data_dir=str(DATA), bitcoin_network=rgb_lib.BitcoinNetwork.SIGNET_CUSTOM, database_type=rgb_lib.DatabaseType.SQLITE,
        max_allocations_per_utxo=5,
        supported_schemas=[rgb_lib.AssetSchema.NIA, rgb_lib.AssetSchema.CFA, rgb_lib.AssetSchema.IFA, rgb_lib.AssetSchema.UDA])


def open_wallet() -> rgb_lib.Wallet:
    meta = json.loads(META.read_text())
    return rgb_lib.Wallet.load(str(DATA), meta["master_fingerprint"], SEED.read_text().strip())


def online(wallet: rgb_lib.Wallet):
    return wallet.go_online(rgb_lib.OnlineOptions(indexer_url=INDEXER, skip_consistency_check=False, vanilla_sync_lookback=20))


def cmd_init(_: argparse.Namespace) -> None:
    if META.exists():
        sys.exit(f"test wallet already exists in {DATA}")
    keys = rgb_lib.generate_keys(rgb_lib.BitcoinNetwork.SIGNET_CUSTOM, rgb_lib.WitnessVersion.TAPROOT)
    SEED.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd = os.open(SEED, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.write(fd, (keys.mnemonic + "\n").encode()); os.close(fd)
    DATA.mkdir(parents=True, mode=0o700)
    wallet = rgb_lib.Wallet(wallet_data(), rgb_lib.SinglesigKeys(
        account_xpub_vanilla=keys.account_xpub_vanilla, account_xpub_colored=keys.account_xpub_colored, vanilla_keychain=None,
        master_fingerprint=keys.master_fingerprint, mnemonic=keys.mnemonic, witness_version=rgb_lib.WitnessVersion.TAPROOT))
    META.write_text(json.dumps({"master_fingerprint": keys.master_fingerprint}))
    address = wallet.get_address()
    LOG.info("test wallet created (fingerprint %s); BTC address %s", keys.master_fingerprint, address)
    record("init", {"master_fingerprint": keys.master_fingerprint, "btc_address": address})


def cmd_invoice(args: argparse.Namespace) -> None:
    wallet = open_wallet()
    expiry = int(time.time()) + 24 * 3600
    data = wallet.witness_receive(args.asset, rgb_lib.Assignment.ANY(), expiry, [PROXY], 1)
    parsed = rgb_lib.Invoice(data.invoice).invoice_data()
    print(data.invoice)
    LOG.info("network %s, endpoints %s, asset %s, recipient %s…", parsed.network, list(parsed.transport_endpoints), parsed.asset_id, data.recipient_id[:24])
    record("invoice", {"invoice": data.invoice, "asset_id": args.asset, "transport_endpoints": list(parsed.transport_endpoints), "network": str(parsed.network)})


def cmd_status(_: argparse.Namespace) -> None:
    wallet = open_wallet()
    on = online(wallet)
    refreshed = wallet.refresh(on, None, [], False)
    assets = wallet.list_assets([])
    rows = []
    for group in (assets.nia or [], assets.cfa or [], assets.ifa or [], assets.uda or []):
        for a in group:
            bal = wallet.get_asset_balance(a.asset_id)
            rows.append({"asset_id": a.asset_id, "ticker": getattr(a, "ticker", None), "name": a.name, "precision": getattr(a, "precision", None),
                         "settled": int(bal.settled), "future": int(bal.future), "spendable": int(bal.spendable)})
    transfers = []
    for t in [x for a in rows for x in wallet.list_transfers(a["asset_id"])]:
        transfers.append({"idx": t.idx, "status": str(t.status).split(".")[-1], "kind": str(t.kind).split(".")[-1],
                          "txid": t.txid, "endpoints": [e.endpoint for e in (t.transport_endpoints or [])]})
    print(json.dumps({"refreshed": {str(k): str(v.updated_status) if hasattr(v, "updated_status") else str(v) for k, v in (refreshed or {}).items()},
                      "assets": rows, "transfers": transfers}, indent=1))
    record("status", {"assets": rows, "transfers": transfers})


def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s", datefmt="%H:%M:%S")
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("init").set_defaults(fn=cmd_init)
    inv = sub.add_parser("invoice"); inv.add_argument("--asset", default=None, help="RGB asset id (rgb:…); omit for any asset"); inv.set_defaults(fn=cmd_invoice)
    sub.add_parser("status").set_defaults(fn=cmd_status)
    args = p.parse_args(); args.fn(args)


if __name__ == "__main__":
    main()
