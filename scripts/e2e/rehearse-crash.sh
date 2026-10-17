#!/usr/bin/env bash
# Rehearsal of the stage moment "keys in a box": a payout from the app, the organizer's micro-VM dies right
# after it sends, a fresh VM boots on the same disk, and the app's Track screen reaches SETTLED by itself.
# Paid exactly once. Test money only (Mutinynet).
#   scripts/e2e/rehearse-crash.sh [amount] [ticker]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
AMOUNT="${1:-1}"; TICKER="${2:-USDT}"
LOG="$ROOT/artifacts/run/logs/rehearse-crash.log"; PIDS="$ROOT/artifacts/run/pids"
t0=$SECONDS; say() { printf '[%3ss] %s\n' "$((SECONDS - t0))" "$*" | tee -a "$LOG"; }
: > "$LOG"
[[ -f "$ROOT/datadirs/payer.IN_VM" ]] || { echo "organizer wallet is not in its VM"; exit 1; }

say "organizer VM: reboot with the crash setting (dies right after it sends)"
"$ROOT/scripts/vm/boot.sh" down payer >/dev/null
POWEREDGE_CRASH_AT=after_broadcast "$ROOT/scripts/vm/boot.sh" up payer >/dev/null
CRASH_PID="$(cat "$PIDS/vm-payer.pid")"; say "organizer VM up (pid $CRASH_PID)"

say "app: crew request -> describe -> review -> approve"
(cd "$ROOT/scripts/e2e" && node app-payout.mjs --amount "$AMOUNT" --ticker "$TICKER" --prefix crash- \
   --why "the crash rehearsal" --track-shots 6 >> "$LOG" 2>&1) &
APP=$!

while kill -0 "$CRASH_PID" 2>/dev/null; do sleep 0.5; (( SECONDS - t0 < 600 )) || { say "VM never died"; exit 1; }; done
say "organizer VM $CRASH_PID is gone (sent, journal not updated)"
"$ROOT/scripts/vm/boot.sh" up payer >/dev/null
say "fresh organizer VM up (pid $(cat "$PIDS/vm-payer.pid")), same data disk, crash setting off"

wait "$APP" && say "app: Track reached SETTLED by itself" || { say "app did not reach SETTLED"; exit 1; }
grep -E 'settled:' "$LOG" | tail -1 | cut -c1-220
