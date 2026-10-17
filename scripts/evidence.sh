#!/usr/bin/env bash
# Submission evidence -> artifacts/evidence/ (read-only: nothing is sent, approved or changed).
#   scripts/evidence.sh            everything, including one fresh QVAC draft (needs the app running)
#   scripts/evidence.sh --no-draft skip the QVAC draft
# Never writes secrets: wallet calls use the token but only their JSON answers are saved.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/artifacts/evidence"; mkdir -p "$OUT"
T="$(cat "$ROOT/secrets/wallet-api.token")"
get() { curl -s --max-time 60 -H "Authorization: Bearer $T" "$1"; }
log() { printf '[evidence] %s\n' "$*"; }

log "dated commit list"
git -C "$ROOT" log --since=2026-10-14 --date=format:'%Y-%m-%d %H:%M %z' --pretty='%ad  %h  %an  %s' > "$OUT/commits.txt"

log "service status"
"$ROOT/scripts/demo-up.sh" status 2>&1 | sed 's/\x1b\[[0-9;]*m//g' > "$OUT/status.txt" || true

log "payouts and statistics (organizer journal)"
python3 - "$OUT" <<EOF
import json, subprocess, sys
out = sys.argv[1]
def get(url):
    r = subprocess.run(["curl", "-s", "--max-time", "60", "-H", "Authorization: Bearer $T", url], capture_output=True, text=True)
    return json.loads(r.stdout or "null")
plans = get("http://127.0.0.1:8711/plans?limit=50")
stats = get("http://127.0.0.1:8711/stats?hours=72")
json.dump({"plans": plans["plans"] if plans else None, "stats72h": stats}, open(f"{out}/payouts.json", "w"), indent=1)
funds = {w: get(f"http://127.0.0.1:{p}/assets") for w, p in (("organizer", 8711), ("crew", 8712))}
json.dump(funds, open(f"{out}/balances.json", "w"), indent=1)
json.dump({"balance": get("http://127.0.0.1:8713/balance")}, open(f"{out}/treasury.json", "w"), indent=1)
EOF

log "key boxes, as Firecracker reports them"
python3 - "$OUT" <<'EOF'
import json, subprocess, sys
out = sys.argv[1]
boxes = {}
for name, role in (("organizer", "payer"), ("crew", "recipient"), ("treasury", "wdk")):
    sock = f"/tmp/scratchmypay-vm/{role}/api.sock"
    def fc(path):
        r = subprocess.run(["curl", "-s", "--max-time", "3", "--unix-socket", sock, f"http://localhost{path}"], capture_output=True, text=True)
        try:
            return json.loads(r.stdout)
        except ValueError:
            return None
    inst, cfg = fc("/"), fc("/vm/config")
    boxes[name] = {
        "instance": inst,
        "networkInterfaces": len(cfg["network-interfaces"]) if cfg else None,
        "drives": [{"id": d["drive_id"], "readOnly": d["is_read_only"], "root": d["is_root_device"]} for d in cfg["drives"]] if cfg else None,
        "machine": {k: cfg["machine-config"][k] for k in ("vcpu_count", "mem_size_mib")} if cfg else None,
        "vsockCid": cfg["vsock"]["guest_cid"] if cfg else None,
    }
json.dump(boxes, open(f"{out}/keys-in-a-box.json", "w"), indent=1)
EOF
"$ROOT/scripts/vm/boot.sh" status > "$OUT/keys-in-a-box.txt" 2>&1 || true
{ echo "# host processes holding keys or seeds (expected: none)"; ps -eo pid,args | grep -E 'poweredge_wallet|wdk-btc/server.mjs|mnemonic' | grep -v -E 'grep|ps -eo' || echo "none"; echo "# host processes with secrets/ open (expected: none)"; lsof +D "$ROOT/secrets" 2>/dev/null | tail -n +2 || true; } >> "$OUT/keys-in-a-box.txt"

log "outbound connections recorded by n2link (egress observe)"
{ echo "# egress decisions by purpose / protocol / port (from artifacts/run/logs/n2link.log)"
  grep -o 'egress decision mode=[a-z]* action=[a-z]* purpose=[^ ]* protocol=[^ ]* port=[0-9]*' "$ROOT/artifacts/run/logs/n2link.log" 2>/dev/null | sort | uniq -c | sort -rn || echo "none"; } > "$OUT/egress.txt"

if [[ "${1:-}" != "--no-draft" ]]; then
  log "one fresh QVAC draft (nothing approved or sent)"
  (cd "$ROOT/scripts/e2e" && node evidence-draft.mjs --out artifacts/evidence/qvac-draft.json) || log "draft capture failed (is the app running?)"
fi

cat > "$OUT/README.md" <<EOF
# Submission evidence (generated $(date -Is) by scripts/evidence.sh)

Mapping of \`docs/ACCEPTANCE.md\` "Submission demonstration" to files. Test network (Mutinynet) only.

| Item | Where |
|---|---|
| Prompt and QVAC's actual structured response (intent, explanation, model, latency) | [qvac-draft.json](qvac-draft.json) |
| Selected template ID and digest; validated graph (flow digest, node count) | [qvac-draft.json](qvac-draft.json) (\`template\`, \`flow\`) |
| Editor | [../screenshots/editor-payouts.png](../screenshots/editor-payouts.png), [../screenshots/dash-editor-tab.png](../screenshots/dash-editor-tab.png), [../screenshots/keys-editor-tab.png](../screenshots/keys-editor-tab.png) |
| Plan and approval (journal states, approver, digests) | [payouts.json](payouts.json), [../payouts/](../payouts/) |
| RGB transactions and asset changes | [payouts.json](payouts.json) (txids), [balances.json](balances.json), [../payouts/2026-10-15-first-live-payout.json](../payouts/2026-10-15-first-live-payout.json), [../payouts/2026-10-17-usdt-via-kaleidoswap-proxy.json](../payouts/2026-10-17-usdt-via-kaleidoswap-proxy.json), [../interop/kaleidoswap-2026-10-17.json](../interop/kaleidoswap-2026-10-17.json) |
| Recovery evidence (paid exactly once after a crash) | [../payouts/2026-10-15-crash-recovery.json](../payouts/2026-10-15-crash-recovery.json), [../payouts/2026-10-17-vm-crash-recovery.json](../payouts/2026-10-17-vm-crash-recovery.json) |
| Keys in a box (Firecracker's own view: 0 network interfaces, read-only disks) | [keys-in-a-box.json](keys-in-a-box.json), [keys-in-a-box.txt](keys-in-a-box.txt) |
| Treasury (WDK) and door | [treasury.json](treasury.json) |
| Outbound connections recorded | [egress.txt](egress.txt) |
| Services at capture time | [status.txt](status.txt) |
| Dated commit list | [commits.txt](commits.txt) |
| Screenshots | [../screenshots/](../screenshots/) |
EOF
log "done: $OUT"
ls -la "$OUT" | tail -n +2
