"""Run one wallet-owner process.

  set -a; . ./.env.payer; set +a
  services/wallet-owner/.venv/bin/python -m poweredge_wallet --port 8711

Env: the Bill-format wallet env (.env.payer / .env.recipient), POWEREDGE_NETWORK=mutinynet,
WALLET_API_TOKEN_FILE (default secrets/wallet-api.token), optional PEER_URL (payer delivers
consignments to the recipient service), optional POWEREDGE_CRASH_AT=before_send|after_broadcast|after_send (demo/test only).
"""

from __future__ import annotations

import argparse
import json
import logging
import fcntl
import os
import secrets
import sys
import threading
import time
import urllib.request
from pathlib import Path
from typing import Any, Callable

import uvicorn

from .api import create_app
from .bill import REPO_ROOT, open_wallet
from .journal import Journal
from . import proxy
from .service import ServiceConfig, WalletService, encode_consignment

LOG = logging.getLogger("poweredge.wallet.main")


def _token(path: Path) -> str:
    if not path.exists():
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as handle:
            handle.write(secrets.token_hex(32) + "\n")
        LOG.info("created wallet API token at %s (git-ignored)", path)
    return path.read_text(encoding="utf-8").strip()


def _deliverer(peer_url: str, token: str) -> Callable[[bytes], dict[str, Any]]:
    def deliver(payload: bytes) -> dict[str, Any]:
        body = json.dumps({"consignment_b64": encode_consignment(payload)}).encode()
        request = urllib.request.Request(
            f"{peer_url.rstrip('/')}/consignments/import", data=body, method="POST",
            headers={"Content-Type": "application/json", "Authorization": f"Bearer {token}"})
        with urllib.request.urlopen(request, timeout=180) as response:
            return json.load(response)
    return deliver


def _auto_refresh(service: WalletService, interval: float) -> None:
    """Background loop: settle incoming transfers (recipient); drive in-flight payouts (payer)."""
    first = True
    while True:
        time.sleep(1 if first else interval)   # first tick right after start: recover without waiting
        first = False
        try:
            result = service.auto_refresh_tick()
            if result and result["updated_transfers"]:
                LOG.info("auto-refresh updated transfers %s", result["updated_transfers"])
            if result and result.get("payouts"):
                LOG.info("auto-refresh payouts in flight %s", result["payouts"])
        except Exception as exc:  # noqa: BLE001 - keep the loop alive; next tick retries
            LOG.warning("auto-refresh failed: %s", exc)


def clear_stale_runtime_lock(data_dir: Path) -> list[Path]:
    """Remove rgb-lib's rgb_runtime.lock when it is provably stale.

    rgb-lib holds that file for the duration of an operation and waits, indefinitely, while it exists.
    If the process that held it stopped mid-operation (e.g. a VM shut down during a refresh), the file
    stays and the next start hangs. Bill's per-wallet process lock (an exclusive flock) tells us whether
    any other process owns this wallet: if we can take it, nobody can be inside the runtime, so the file
    is stale. If we cannot take it, another process owns the wallet and we leave everything alone.
    """
    data_dir = data_dir.resolve()
    stale = sorted(data_dir.glob("*/rgb_runtime.lock"))
    if not stale:
        return []
    owner_lock = data_dir.parent / f".{data_dir.name}.rgb-bill.lock"
    with owner_lock.open("a+", encoding="utf-8") as handle:
        try:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return []
        try:
            for path in stale:
                LOG.warning("removing stale rgb-lib runtime lock %s (no other process owns this wallet)", path)
                path.unlink(missing_ok=True)
        finally:
            fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
    return stale


def main() -> int:
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s: %(message)s")
    parser = argparse.ArgumentParser(description="PowerEdge wallet-owner service")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, required=True)
    args = parser.parse_args()

    if args.host not in {"127.0.0.1", "::1", "localhost"}:
        LOG.error("refusing to bind %s: the wallet service is loopback-only", args.host)
        return 2
    if os.environ.get("POWEREDGE_NETWORK") != "mutinynet":
        LOG.error("POWEREDGE_NETWORK must be 'mutinynet'; refusing to start")
        return 2
    role = os.environ.get("POWEREDGE_ROLE", "")
    if role not in {"payer", "recipient"}:
        LOG.error("POWEREDGE_ROLE must be payer or recipient")
        return 2

    token = _token(Path(os.environ.get("WALLET_API_TOKEN_FILE", REPO_ROOT / "secrets" / "wallet-api.token")))
    data_dir = os.environ.get(f"RGB_DATA_DIR_{os.environ.get('BILL_ROLE', '')}")
    if data_dir:
        clear_stale_runtime_lock(Path(data_dir))
    for attempt in range(6):
        try:
            settings, adapter, ops, errors, _backend = open_wallet()
            break
        except Exception as exc:  # noqa: BLE001
            if "429" not in str(exc) or attempt == 5:
                LOG.error("cannot open wallet: %s", exc)
                return 1
            delay = 10 * (attempt + 1)
            LOG.warning("indexer rate-limited while opening wallet; retrying in %ss", delay)
            time.sleep(delay)

    peer = os.environ.get("PEER_URL")
    config = ServiceConfig(
        wallet_id=role,
        role=role,
        deliver=_deliverer(peer, token) if (peer and role == "payer") else None,
        crash_at=os.environ.get("POWEREDGE_CRASH_AT") or None,
        indexer_url=os.environ.get("ELECTRUM_URL", "https://mutinynet.com/api"),
        proxy_client=proxy.ProxyClient(),
        allowed_proxies=proxy.allowed_proxies(),
        default_transport=os.environ.get("POWEREDGE_INVOICE_TRANSPORT", "direct"),
    )
    config.tx_outputs = lambda txid: proxy.esplora_outputs(config.indexer_url, txid)
    journal = Journal(REPO_ROOT / "datadirs" / "journal" / f"{role}.sqlite3")
    service = WalletService(adapter, ops, journal, config, errors=errors)
    interval = float(os.environ.get("POWEREDGE_AUTO_REFRESH_SECONDS", "60" if role == "recipient" else "20"))
    if interval > 0:
        threading.Thread(target=_auto_refresh, args=(service, interval), name="auto-refresh", daemon=True).start()
        LOG.info("auto-refresh of pending incoming transfers every %.0fs", interval)
    LOG.info("wallet-owner ready: role=%s bill_role=%s port=%d peer=%s crash_at=%s",
             role, settings.role.value, args.port, peer or "-", config.crash_at or "-")
    uvicorn.run(create_app(service, token), host=args.host, port=args.port, log_level="info")
    # Shutting down: let a wallet operation in progress (e.g. the background refresh) finish, so rgb-lib
    # releases its runtime lock. Holding the service lock also stops a new one from starting.
    LOG.info("shutting down: waiting for any wallet operation in progress")
    if service._lock.acquire(timeout=20):
        LOG.info("wallet idle; exiting cleanly")
    else:
        LOG.warning("a wallet operation did not finish within 20 s; exiting anyway")
    return 0


if __name__ == "__main__":
    sys.exit(main())
