#!/usr/bin/env bash
# Start the whole ScratchMyPay demo in the right order, waiting for each service to be really ready.
#
#   scripts/demo-up.sh            start everything (skips services that are already healthy)
#   scripts/demo-up.sh status     health of every service + payer funds + WDK treasury check
#   scripts/demo-up.sh stop       stop the PowerEdge services (only ours, matched by port + command)
#
# Order: QVAC (local AI), crew wallet, WDK treasury  →  organizer wallet (needs crew wallet)
#        →  n2link (app + editor)  →  QVAC warm-up.
# Everything listens on 127.0.0.1. Logs and PIDs: artifacts/run/ (git-ignored).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN="$ROOT/artifacts/run"
LOGS="$RUN/logs"
PIDS="$RUN/pids"
TOKEN_FILE="$ROOT/secrets/wallet-api.token"
QVAC_KEY_FILE="$ROOT/secrets/qvac.api-key"
PAYOUTS_TONIGHT="${POWEREDGE_PAYOUTS_TONIGHT:-5}"

# name | port | command marker (to recognise our own process) | readiness timeout (s)
SERVICES=(
  "qvac|11435|qvac serve|300"
  "recipient|8712|poweredge_wallet|120"
  "wdk|8713|wdk-btc/server.mjs|60"
  "payer|8711|poweredge_wallet|120"
  "n2link|1888|n2linkd|90"
)

# ------------------------------------------------------------------ helpers
if [[ -t 1 ]]; then G=$'\e[32m'; R=$'\e[31m'; Y=$'\e[33m'; B=$'\e[1m'; N=$'\e[0m'; else G=; R=; Y=; B=; N=; fi
log()  { printf '%s [demo-up] %s\n' "$(date +%H:%M:%S)" "$*"; }
ok()   { log "${G}✓${N} $*"; }
warn() { log "${Y}!${N} $*"; }
die()  { log "${R}✗ $*${N}"; exit 1; }

field() { local spec="$1" i="$2"; IFS='|' read -r -a parts <<<"$spec"; echo "${parts[$i]}"; }
spec_of() { local s; for s in "${SERVICES[@]}"; do [[ "$(field "$s" 0)" == "$1" ]] && { echo "$s"; return; }; done; die "unknown service $1"; }

token() { cat "$TOKEN_FILE"; }
get()  { curl -fsS --max-time "${3:-5}" -H "Authorization: Bearer $2" "$1" 2>/dev/null; }
json() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)" 2>/dev/null; }

pid_on_port() {  # listener PID on 127.0.0.1:<port>, empty if none
  { ss -ltnpH "sport = :$1" 2>/dev/null | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2; } || true
}

# ------------------------------------------------------------------ readiness checks
ready_qvac() {
  [[ -f "$QVAC_KEY_FILE" ]] || return 1
  get "http://127.0.0.1:11435/v1/models" "$(cat "$QVAC_KEY_FILE")" \
    | json "'yes' if any(m.get('state')=='ready' for m in d.get('data',[])) else ''" | grep -q yes
}
ready_wallet() {  # role port
  get "http://127.0.0.1:$2/capabilities" "$(token)" 20 \
    | json "'yes' if d['role']=='$1' and d['environment']=='mutinynet' and d['mainnet'] is False else ''" | grep -q yes
}
ready_recipient() { ready_wallet recipient 8712; }
ready_payer()     { ready_wallet payer 8711; }
ready_wdk()       { curl -fsS --max-time 5 http://127.0.0.1:8713/health 2>/dev/null | grep -q '"ok":true'; }
ready_n2link()    { curl -fsS --max-time 10 -o /dev/null http://127.0.0.1:1888/poweredge-app/login 2>/dev/null; }

# ------------------------------------------------------------------ start / wait
launch() {  # name — start detached in its own process group
  local name="$1" cmd
  case "$name" in
    qvac)      cmd=("$ROOT/services/qvac/serve.sh") ;;
    recipient) cmd=("$ROOT/services/wallet-owner/run.sh" recipient) ;;
    payer)     cmd=("$ROOT/services/wallet-owner/run.sh" payer) ;;
    wdk)       cmd=(node "$ROOT/services/wdk-btc/server.mjs") ;;
    n2link)    cmd=("$ROOT/scripts/run-n2link.sh") ;;
  esac
  if "ready_$name"; then ok "$name already running"; return; fi
  local spec port busy
  spec="$(spec_of "$name")"; port="$(field "$spec" 1)"
  busy="$(pid_on_port "$port")"
  [[ -z "$busy" ]] || die "port $port is held by PID $busy ($(tr '\0' ' ' </proc/"$busy"/cmdline 2>/dev/null | cut -c1-80)) but $name is not healthy; stop it first"
  printf "\n===== %s start %s =====\n" "$name" "$(date -Is)" >>"$LOGS/$name.log"
  setsid nohup "${cmd[@]}" >>"$LOGS/$name.log" 2>&1 </dev/null &
  echo $! >"$PIDS/$name.pid"
  log "started $name (pid $!, log artifacts/run/logs/$name.log)"
}

wait_ready() {  # name
  local name="$1" spec timeout start
  spec="$(spec_of "$name")"; timeout="$(field "$spec" 3)"; start=$SECONDS
  until "ready_$name"; do
    if [[ -f "$PIDS/$name.pid" ]] && ! kill -0 "$(cat "$PIDS/$name.pid")" 2>/dev/null; then
      tail -n 15 "$LOGS/$name.log" >&2 || true
      die "$name exited during start-up (log above)"
    fi
    (( SECONDS - start < timeout )) || { tail -n 15 "$LOGS/$name.log" >&2 || true; die "$name not ready after ${timeout}s (log above)"; }
    sleep 2
  done
  ok "$name ready ($(( SECONDS - start ))s)"
}

# ------------------------------------------------------------------ pre-flight
preflight() {
  local missing=()
  for f in secrets/wallet-api.token secrets/payer.mnemonic secrets/recipient.mnemonic .env.payer .env.recipient \
           services/wallet-owner/.venv/bin/python services/qvac/node_modules services/wdk-btc/node_modules \
           n2link/target/debug/n2linkd; do
    [[ -e "$ROOT/$f" ]] || missing+=("$f")
  done
  (( ${#missing[@]} == 0 )) || die "missing: ${missing[*]} (see README 'Install and run')"
  for bin in curl python3 node ss setsid; do command -v "$bin" >/dev/null || die "$bin not found"; done
  local node_major; node_major="$(node -p 'process.versions.node.split(".")[0]')"
  (( node_major >= 22 )) || die "Node $node_major found; QVAC and WDK need Node 22+"
  [[ -f "$ROOT/n2link/target/ui_static/red/red.min.js" ]] || warn "flow editor not built (run scripts/setup-editor.sh); the guided app still works"
  [[ -d "${BILL_ROOT:-$ROOT/../bill}" ]] || die "Bill (prior work) not found at ${BILL_ROOT:-$ROOT/../bill}"
  ok "pre-flight checks passed"
}

warm_up_qvac() {  # load the model into GPU memory so the first draft on stage is fast
  local start=$SECONDS
  curl -fsS --max-time 120 -o /dev/null -H "Authorization: Bearer $(cat "$QVAC_KEY_FILE")" -H 'Content-Type: application/json' \
    -d '{"model":"poweredge-intent","messages":[{"role":"user","content":"ok"}],"max_tokens":1}' \
    http://127.0.0.1:11435/v1/chat/completions \
    && ok "QVAC warmed up ($(( SECONDS - start ))s)" || warn "QVAC warm-up failed (first draft may be slower)"
}

# ------------------------------------------------------------------ status
report() {
  local s name port
  printf '\n%s%-10s %-6s %-8s %s%s\n' "$B" "SERVICE" "PORT" "STATE" "PID" "$N"
  for s in "${SERVICES[@]}"; do
    name="$(field "$s" 0)"; port="$(field "$s" 1)"
    if "ready_$name"; then printf '%-10s %-6s %s%-8s%s %s\n' "$name" "$port" "$G" "ready" "$N" "$(pid_on_port "$port")"
    else printf '%-10s %-6s %s%-8s%s %s\n' "$name" "$port" "$R" "DOWN" "$N" "$(pid_on_port "$port")"; fi
  done
  echo
  if ready_payer; then
    get http://127.0.0.1:8711/assets "$(token)" 30 | json "'\n'.join('Organizer funds: {:,.{p}f} {} spendable'.format(a['spendable'] / 10 ** a['precision'], a['ticker'], p=min(a['precision'], 2)) for a in d['assets']) or 'Organizer funds: no assets'" || warn "could not read organizer funds"
    get "http://127.0.0.1:8711/funds?fee_rate=2" "$(token)" 90 \
      | json "'Organizer sats:  {:,} plain + {:,} on RGB outputs; {} free RGB outputs; ~{} payouts at 2 sat/vB'.format(d['btc']['vanilla']['spendable'], d['btc']['colored']['spendable'], d['utxos']['free_colorable'], d['estimate']['payouts_from_colored_sats'])" \
      || warn "could not read organizer outputs"
  fi
  if ready_recipient; then
    get http://127.0.0.1:8712/assets "$(token)" 30 | json "'\n'.join('Crew funds:      {:,.2f} {} spendable'.format(a['spendable'] / 10 ** a['precision'], a['ticker']) for a in d['assets']) or 'Crew funds:      none yet'" || warn "could not read crew funds"
  fi
  if ready_wdk; then
    get "http://127.0.0.1:8713/door/passes" "$(token)" 30 \
      | json "'Door revenue:    {:,} sats from {} passes ({} active) at {:,} sats each'.format(d['revenueSat'], len(d['passes']) if isinstance(d['passes'], list) else d['passes'], d['activePasses'], d['priceSat'])" \
      || warn "could not read door revenue"
    get "http://127.0.0.1:8713/check?count=$PAYOUTS_TONIGHT" "$(token)" 30 \
      | json "(lambda c, a: 'WDK treasury:    {:,} sats; {} payouts need {:,} sats -> {}'.format(c['haveSat'], c['payoutsTonight'], c['needSat'], 'OK' if c['enough'] else 'TOP UP (short {:,} sats; fund {})'.format(c['shortSat'], a)))(d['coverage'], d['address'])" \
      || warn "could not read WDK treasury"
  fi
  echo
  echo "App:    http://127.0.0.1:1888/poweredge-app"
  echo "Editor: http://127.0.0.1:1888/"
}

# ------------------------------------------------------------------ stop
stop_one() {  # name — only stops a process we recognise as ours
  local name="$1" spec port marker pid pgid
  spec="$(spec_of "$name")"; port="$(field "$spec" 1)"; marker="$(field "$spec" 2)"
  pid="$(pid_on_port "$port")"
  if [[ -z "$pid" ]]; then rm -f "$PIDS/$name.pid"; log "$name not running"; return; fi
  if ! tr '\0' ' ' </proc/"$pid"/cmdline 2>/dev/null | grep -q -- "$marker"; then
    warn "port $port is held by PID $pid, which is not $name; leaving it alone"; return
  fi
  if [[ -f "$PIDS/$name.pid" ]] && pgid="$(ps -o pgid= -p "$(cat "$PIDS/$name.pid")" 2>/dev/null | tr -d ' ')" \
     && [[ -n "$pgid" && "$pgid" == "$(cat "$PIDS/$name.pid")" ]]; then
    kill -TERM -- "-$pgid" 2>/dev/null || true          # our own process group (started by demo-up)
  else
    kill -TERM "$pid" 2>/dev/null || true                # started by hand: stop the listener only
  fi
  for _ in $(seq 1 20); do [[ -z "$(pid_on_port "$port")" ]] && break; sleep 0.5; done
  if [[ -n "$(pid_on_port "$port")" ]]; then kill -KILL "$pid" 2>/dev/null || true; warn "$name did not stop in 10s; killed"; fi
  rm -f "$PIDS/$name.pid"
  ok "$name stopped"
}

# ------------------------------------------------------------------ main
mkdir -p "$LOGS" "$PIDS"
case "${1:-up}" in
  up)
    preflight
    launch qvac; launch recipient; launch wdk          # independent: start together
    wait_ready recipient
    launch payer                                       # delivers consignments to the crew wallet
    wait_ready payer; wait_ready wdk; wait_ready qvac
    launch n2link; wait_ready n2link
    warm_up_qvac
    report ;;
  status) report ;;
  stop)
    for name in n2link payer wdk recipient qvac; do stop_one "$name"; done ;;
  *) echo "usage: $0 [up|status|stop]" >&2; exit 2 ;;
esac
