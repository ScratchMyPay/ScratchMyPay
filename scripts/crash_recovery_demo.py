#!/usr/bin/env python3
"""Live crash-recovery run (Mutinynet, test money only).

Phase 1  `crash`   : crew invoice -> plan -> approve -> execute. The organizer service must be
                     running with POWEREDGE_CRASH_AT=after_broadcast, so it dies right after the
                     wallet broadcasts and BEFORE the journal records it (worst moment).
(restart the organizer service WITHOUT the crash setting)
Phase 2  `recover` : show the restart marked the payout UNKNOWN_SUBMISSION, show a second execute
                     is refused (never resends), reconcile from wallet evidence, deliver, wait for
                     SETTLED and for the crew wallet to settle by itself. Writes evidence JSON to
                     artifacts/payouts/.

Usage:
  scripts/crash_recovery_demo.py crash   [--amount 1]
  scripts/crash_recovery_demo.py recover [--timeout 1800]
  --vm: keys-in-a-box edition. crash reboots the organizer micro-VM with the crash setting, and the
        VM exits after broadcast; recover boots a NEW organizer VM on the same data disk. The journal
        is read through the wallet API (it lives on the VM's disk).
"""
from __future__ import annotations

import argparse
import json
import logging
import os
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
PAYER, CREW = "http://127.0.0.1:8711", "http://127.0.0.1:8712"
TOKEN = (ROOT / "secrets" / "wallet-api.token").read_text().strip()
STATE_FILE = ROOT / "artifacts" / "run" / "crash-demo.json"   # git-ignored scratch
JOURNAL = ROOT / "datadirs" / "journal" / "payer.sqlite3"
TEMPLATE_DIGEST = "2a68da911b458db3ea616024ae2e88c1e597480e1792e9086860e12ea4d68270"
TICKER = "TUSDT"
LOG = logging.getLogger("crash-demo")
VM_MODE = False
BOOT = ROOT / "scripts" / "vm" / "boot.sh"
VM_PID = ROOT / "artifacts" / "run" / "pids" / "vm-payer.pid"


def vm_pid() -> int | None:
    try:
        pid = int(VM_PID.read_text())
        os.kill(pid, 0)
        return pid
    except (OSError, ValueError):
        return None


def boot_organizer_vm(crash_at: str | None) -> int:
    env = dict(os.environ)
    env.pop("POWEREDGE_CRASH_AT", None)
    if crash_at:
        env["POWEREDGE_CRASH_AT"] = crash_at
    subprocess.run([str(BOOT), "down", "payer"], check=True, env=env)
    subprocess.run([str(BOOT), "up", "payer"], check=True, env=env)
    pid = vm_pid()
    LOG.info("organizer micro-VM booted (pid %s, crash setting %s)", pid, crash_at or "off")
    return pid


def call(base: str, method: str, path: str, body: dict | None = None, timeout: float = 180) -> tuple[int, Any]:
    req = urllib.request.Request(
        base + path, method=method, data=None if body is None else json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {TOKEN}"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, json.load(resp)
    except urllib.error.HTTPError as exc:
        return exc.code, json.loads(exc.read() or b"{}")


def must(result: tuple[int, Any], what: str) -> Any:
    status, body = result
    if status != 200:
        raise SystemExit(f"{what} failed: HTTP {status} {body}")
    return body


def journal(plan_id: str) -> dict[str, Any]:
    """Read-only view of the plan row and its event history."""
    if VM_MODE:   # the journal lives on the VM's disk; read it through the wallet API
        body = must(call(PAYER, "GET", f"/plans/{plan_id}"), "plan")
        events = [{"at": datetime.fromtimestamp(e["at"], timezone.utc).isoformat(timespec="seconds"),
                   "from_state": e["from_state"], "to_state": e["to_state"], "detail": e["detail"]}
                  for e in body.get("events", [])]
        plan = {k: body.get(k) for k in ("state", "txid", "idempotency_key", "consignment_sha256")}
        return {"plan": plan, "events": events}
    db = sqlite3.connect(f"file:{JOURNAL}?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    row = db.execute("SELECT state, txid, idempotency_key, consignment_sha256 FROM plans WHERE plan_id=?",
                     (plan_id,)).fetchone()
    events = [dict(e) for e in db.execute(
        "SELECT at, from_state, to_state, detail FROM events WHERE plan_id=? ORDER BY id", (plan_id,))]
    db.close()
    for e in events:
        e["at"] = datetime.fromtimestamp(e["at"], timezone.utc).isoformat(timespec="seconds")
    return {"plan": dict(row) if row else None, "events": events}


def crew_balance() -> int:
    assets = must(call(CREW, "GET", "/assets"), "crew assets")["assets"]
    return next((a["spendable"] for a in assets if a["ticker"] == TICKER), 0)


def phase_crash(amount: str) -> None:
    from decimal import Decimal
    asset = next(a for a in must(call(PAYER, "GET", "/assets"), "payer assets")["assets"] if a["ticker"] == TICKER)
    base_units = int(Decimal(amount) * 10 ** asset["precision"])
    crew_before = crew_balance()
    crashed_vm = boot_organizer_vm("after_broadcast") if VM_MODE else None
    invoice = must(call(CREW, "POST", "/invoices", {"asset_id": asset["asset_id"], "expires_seconds": 3600}), "invoice")
    LOG.info("crew invoice created (recipient %s…)", invoice["recipient_id"][:16])
    revision = f"crash-demo-{int(time.time())}"
    plan = must(call(PAYER, "POST", "/plans", {
        "invoice": invoice["invoice"], "asset_ticker": TICKER, "amount_base_units": base_units,
        "amount_display": amount, "recipient_label": "crash-recovery test", "memo": "live crash-recovery run",
        "flow_revision": revision, "template_digest": TEMPLATE_DIGEST}), "plan")
    LOG.info("plan %s: %s %s to the crew", plan["plan_id"], amount, TICKER)
    must(call(PAYER, "POST", f"/plans/{plan['plan_id']}/approve", {
        "plan_digest": plan["plan_digest"], "flow_revision": revision, "actor": "Gus (crash demo)"}), "approve")
    LOG.info("approved; executing with the organizer service set to crash after broadcast…")
    STATE_FILE.parent.mkdir(parents=True, exist_ok=True)
    STATE_FILE.write_text(json.dumps({"plan_id": plan["plan_id"], "flow_revision": revision, "amount": amount,
                                      "base_units": base_units, "crew_before": crew_before, "crashed_vm_pid": crashed_vm,
                                      "started_at": datetime.now(timezone.utc).isoformat(timespec="seconds")}))
    try:
        status, body = call(PAYER, "POST", f"/plans/{plan['plan_id']}/execute",
                            {"flow_revision": revision, "idempotency_key": f"{plan['plan_id']}-first"})
        LOG.error("expected the service to die, but it answered HTTP %s: %s", status, body)
        sys.exit(1)
    except (urllib.error.URLError, ConnectionError, OSError) as exc:
        LOG.info("organizer service died mid-request as intended (%s)", type(exc).__name__)
    if VM_MODE:
        for _ in range(60):
            if vm_pid() is None:
                break
            time.sleep(0.5)
        LOG.info("organizer micro-VM pid %s %s", crashed_vm, "is gone" if vm_pid() is None else "IS STILL RUNNING")
        return
    snap = journal(plan["plan_id"])
    LOG.info("journal after crash: state=%s, txid=%s", snap["plan"]["state"], snap["plan"]["txid"])
    print(json.dumps(snap, indent=1))


def phase_recover(timeout: int) -> None:
    st = json.loads(STATE_FILE.read_text())
    plan_id, revision = st["plan_id"], st["flow_revision"]
    new_vm = boot_organizer_vm(None) if VM_MODE else None
    caps = must(call(PAYER, "GET", "/capabilities"), "capabilities")
    LOG.info("after restart the service reports recovered submissions: %s", caps["recovered_unknown_submissions"])
    after_restart = journal(plan_id)["plan"]["state"]

    status, again = call(PAYER, "POST", f"/plans/{plan_id}/execute",
                         {"flow_revision": revision, "idempotency_key": f"{plan_id}-second"})
    LOG.info("second execute -> HTTP %s, state %s, already_executed=%s",
             status, again.get("state"), again.get("already_executed"))

    reconciled = must(call(PAYER, "POST", f"/plans/{plan_id}/reconcile"), "reconcile")
    LOG.info("reconciled from wallet evidence: state=%s txid=%s", reconciled["state"], reconciled.get("txid"))
    must(call(PAYER, "POST", f"/plans/{plan_id}/deliver"), "deliver")

    deadline = time.time() + timeout
    state = None
    while time.time() < deadline:
        state = must(call(PAYER, "GET", f"/plans/{plan_id}?refresh=true"), "status")["state"]
        LOG.info("organizer side: %s", state)
        if state in ("SETTLED", "FAILED"):
            break
        time.sleep(30)
    crew_after = st["crew_before"]
    while time.time() < deadline:
        crew_after = crew_balance()
        LOG.info("crew wallet spendable: %s (before %s)", crew_after, st["crew_before"])
        if crew_after >= st["crew_before"] + st["base_units"]:
            break
        time.sleep(30)

    snap = journal(plan_id)
    sends = [e for e in snap["events"] if e["to_state"] in ("SUBMITTING",)]
    evidence = {
        "what": "Live crash-recovery run: organizer service killed after broadcast, before journaling",
        "network": "mutinynet (test money only)",
        "crash_point": "after_broadcast (POWEREDGE_CRASH_AT)",
        "micro_vm": ({"crashed_vm_pid": st.get("crashed_vm_pid"), "recovered_on_new_vm_pid": new_vm,
                      "note": "organizer wallet in a Firecracker micro-VM; the VM exited after broadcast and a new VM booted on the same data disk"}
                     if VM_MODE else None),
        "plan_id": plan_id, "amount": f"{st['amount']} {TICKER}",
        "started_at": st["started_at"],
        "state_after_restart": after_restart,
        "recovered_by_service": caps["recovered_unknown_submissions"],
        "second_execute": {"http_status": status, "state": again.get("state"),
                           "already_executed": again.get("already_executed")},
        "reconciled_txid": reconciled.get("txid"),
        "final_state": state,
        "submissions_in_journal": len(sends),
        "crew_balance_base_units": {"before": st["crew_before"], "after": crew_after},
        "journal": snap,
    }
    out = ROOT / "artifacts" / "payouts" / f"{datetime.now().date().isoformat()}-{'vm-' if VM_MODE else ''}crash-recovery.json"
    out.write_text(json.dumps(evidence, indent=1) + "\n")
    LOG.info("evidence written to %s", out.relative_to(ROOT))
    ok = (after_restart == "UNKNOWN_SUBMISSION" and again.get("already_executed")
          and reconciled.get("txid") and len(sends) == 1 and state == "SETTLED"
          and crew_after >= st["crew_before"] + st["base_units"])
    LOG.info("RESULT: %s", "PASS - paid exactly once, recovered after crash" if ok else "CHECK EVIDENCE")
    sys.exit(0 if ok else 1)


def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s", datefmt="%H:%M:%S")
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("phase", choices=["crash", "recover"])
    parser.add_argument("--amount", default="1")
    parser.add_argument("--timeout", type=int, default=1800)
    parser.add_argument("--vm", action="store_true", help="organizer wallet runs in its micro-VM")
    args = parser.parse_args()
    global VM_MODE
    VM_MODE = args.vm
    phase_crash(args.amount) if args.phase == "crash" else phase_recover(args.timeout)


if __name__ == "__main__":
    main()
