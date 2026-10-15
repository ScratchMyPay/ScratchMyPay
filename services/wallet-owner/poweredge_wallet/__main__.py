"""Run one wallet-owner process.

  set -a; . ./.env.payer; set +a
  services/wallet-owner/.venv/bin/python -m poweredge_wallet --port 8711

Env: the Bill-format wallet env (.env.payer / .env.recipient), POWEREDGE_NETWORK=mutinynet,
WALLET_API_TOKEN_FILE (default secrets/wallet-api.token), optional PEER_URL (payer delivers
consignments to the recipient service), optional POWEREDGE_CRASH_AT=before_send|after_send.
"""

from __future__ import annotations

import argparse
import json
import logging
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
    """Background loop: settle incoming transfers once they confirm (recipient)."""
    while True:
        time.sleep(interval)
        try:
            result = service.auto_refresh_tick()
            if result and result["updated_transfers"]:
                LOG.info("auto-refresh updated transfers %s", result["updated_transfers"])
        except Exception as exc:  # noqa: BLE001 - keep the loop alive; next tick retries
            LOG.warning("auto-refresh failed: %s", exc)


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
    )
    journal = Journal(REPO_ROOT / "datadirs" / "journal" / f"{role}.sqlite3")
    service = WalletService(adapter, ops, journal, config, errors=errors)
    interval = float(os.environ.get("POWEREDGE_AUTO_REFRESH_SECONDS", "60" if role == "recipient" else "0"))
    if interval > 0:
        threading.Thread(target=_auto_refresh, args=(service, interval), name="auto-refresh", daemon=True).start()
        LOG.info("auto-refresh of pending incoming transfers every %.0fs", interval)
    LOG.info("wallet-owner ready: role=%s bill_role=%s port=%d peer=%s crash_at=%s",
             role, settings.role.value, args.port, peer or "-", config.crash_at or "-")
    uvicorn.run(create_app(service, token), host=args.host, port=args.port, log_level="info")
    return 0


if __name__ == "__main__":
    sys.exit(main())
