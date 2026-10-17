#!/usr/bin/env bash
# Keys in a box: run one wallet (payer = organizer, recipient = crew) in a Firecracker micro-VM.
#   scripts/vm/boot.sh up payer|recipient      boot the VM + host bridge, wait for /health
#   scripts/vm/boot.sh down payer|recipient    stop the VM and its bridge (data stays on the VM's disk)
#   scripts/vm/boot.sh return payer|recipient  VM stopped: copy wallet data back to datadirs/, host owns it again
#   scripts/vm/boot.sh status
# Ownership: while datadirs/<role>.IN_VM exists the VM's data disk is the only live copy of that wallet;
# services/wallet-owner/run.sh refuses to start on the host. Never two live copies of one wallet.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VM="$ROOT/.vm"; RUN="/tmp/scratchmypay-vm"; LOGS="$ROOT/artifacts/run/logs"; PIDS="$ROOT/artifacts/run/pids"
log() { printf '[vm %s] %s\n' "${ROLE:-*}" "$*"; }
die() { printf '[vm %s] ERROR: %s\n' "${ROLE:-*}" "$*" >&2; exit 1; }

CMD="${1:-status}"; ROLE="${2:-}"
port_of() { case "$1" in payer) echo 8711 ;; recipient) echo 8712 ;; wdk) echo 8713 ;; *) die "role must be payer, recipient or wdk" ;; esac; }
data_of() { case "$1" in wdk) echo door ;; *) echo "$1" ;; esac; }          # datadirs/<name> that moves into the VM
seed_of() { case "$1" in wdk) echo wdk-treasury ;; *) echo "$1" ;; esac; }  # secrets/<name>.mnemonic
kind_of() { case "$1" in wdk) echo treasury ;; *) echo wallet ;; esac; }    # rootfs image
alive() { [[ -f "$PIDS/$1.pid" ]] && kill -0 "$(cat "$PIDS/$1.pid")" 2>/dev/null; }
health() { curl -s --max-time 3 "http://127.0.0.1:$1/health" | grep -q '"ok"' ; }

adopt() {   # first boot: move this wallet's live data from the host into a data-disk image
  local role="$1" stage="$VM/build/data-$1" data; data="$(data_of "$1")"
  alive "$role" && die "host $role service is running; stop it first (scripts/demo-up.sh stop)"
  lsof +D "$ROOT/datadirs/$data" >/dev/null 2>&1 && die "something on the host has datadirs/$data open"
  rm -rf "$stage"; mkdir -p "$stage/datadirs/journal" "$stage/rgb-oob"
  cp -a "$ROOT/datadirs/$data" "$stage/datadirs/"
  cp -a "$ROOT/datadirs/journal/$role".sqlite3* "$stage/datadirs/journal/" 2>/dev/null || true
  [[ -d "$ROOT/.rgb-oob/$role" ]] && cp -a "$ROOT/.rgb-oob/$role" "$stage/rgb-oob/"
  echo "vm:$role" > "$stage/OWNER"
  mkdir -p "$VM/$role"
  mkfs.ext4 -q -F -L "data-$role" -d "$stage" "$VM/$role/data.ext4" 512M
  rm -rf "$stage"
  printf 'live copy is in %s since %s; run scripts/vm/boot.sh return %s to bring it back\n' \
    "$VM/$role/data.ext4" "$(date -Is)" "$role" > "$ROOT/datadirs/$role.IN_VM"
  log "wallet data moved into $VM/$role/data.ext4 (host copy is now stale; run.sh refuses it)"
}

seeds_disk() {   # read-only disk: mnemonic, API token, env with VM paths; built in a private tmp dir
  local role="$1" dir="$RUN/$1/seed" letter; letter=$([[ $role == payer ]] && echo A || echo B)
  rm -rf "$dir"; mkdir -p "$dir"; chmod 0700 "$RUN" "$RUN/$role" "$dir"
  install -m 0400 "$ROOT/secrets/$(seed_of "$role").mnemonic" "$dir/mnemonic"
  install -m 0400 "$ROOT/secrets/wallet-api.token" "$dir/wallet-api.token"
  if [[ $role == wdk ]]; then   # the treasury reads repo-relative paths; only the seed file moves
    printf 'WDK_SEED_FILE=/secrets/mnemonic\nWALLET_API_TOKEN_FILE=/secrets/wallet-api.token\n' > "$dir/env"
    rm -f "$RUN/$role/seeds.ext4"
    (umask 077; mkfs.ext4 -q -F -L "seeds-$role" -d "$dir" "$RUN/$role/seeds.ext4" 4M)
    rm -rf "$dir"; return
  fi
  sed -e "s#$ROOT/datadirs#/opt/poweredge/datadirs#g" \
      -e "s#$ROOT/secrets/$role.mnemonic#/secrets/mnemonic#g" \
      -e "s#$ROOT/.rgb-oob#/data/rgb-oob#g" "$ROOT/.env.$role" > "$dir/env"
  {
    echo "WALLET_API_TOKEN_FILE=/secrets/wallet-api.token"
    echo "BILL_ROOT=/opt/bill"
    echo "POWEREDGE_RGB_PROXY=${POWEREDGE_RGB_PROXY:-rpcs://proxy.kaleidoswap.com/json-rpc}"
    echo "POWEREDGE_INVOICE_TRANSPORT=${POWEREDGE_INVOICE_TRANSPORT:-proxy}"
    [[ $role == payer ]] && echo "PEER_URL=http://127.0.0.1:8712"
    [[ -n "${POWEREDGE_CRASH_AT:-}" ]] && echo "POWEREDGE_CRASH_AT=$POWEREDGE_CRASH_AT"
  } >> "$dir/env"
  grep -q "RGB_MNEMONIC_FILE_$letter=/secrets/mnemonic" "$dir/env" || die "env rewrite failed"
  if grep -q "$ROOT" "$dir/env"; then die "env still names host paths"; fi
  rm -f "$RUN/$role/seeds.ext4"
  (umask 077; mkfs.ext4 -q -F -L "seeds-$role" -d "$dir" "$RUN/$role/seeds.ext4" 4M)
  rm -rf "$dir"
}

bridge() {   # host side of a VM's vsock: API in, pinholes out, organizer -> crew delivery
  local role="$1" port uds="$RUN/$1/v.sock" peer=() holes; port="$(port_of "$role")"
  if [[ $role == wdk ]]; then holes=(--pinhole "$uds:10001=mutinynet.com:443" --pinhole "$uds:10003=electrum.mutinynet.com:50002")
  else holes=(--pinhole "$uds:10001=mutinynet.com:443" --pinhole "$uds:10002=proxy.kaleidoswap.com:443"); fi
  if alive "vm-$role-bridge"; then kill -TERM "$(cat "$PIDS/vm-$role-bridge.pid")" 2>/dev/null || true; sleep 0.3; fi
  if [[ $role == payer ]]; then
    if alive vm-recipient; then peer=(--relay "$uds:8712=$RUN/recipient/v.sock:8712")   # VM -> VM
    else peer=(--pinhole "$uds:8712=127.0.0.1:8712"); fi                                # crew on the host
  fi
  setsid python3 "$ROOT/scripts/vm/vsock-bridge.py" --expose "127.0.0.1:$port=$uds:$port" "${holes[@]}" "${peer[@]}" \
    >> "$LOGS/vm-$role-bridge.log" 2>&1 < /dev/null &
  echo $! > "$PIDS/vm-$role-bridge.pid"
  log "bridge: API 127.0.0.1:$port; pinholes $(printf '%s ' "${holes[@]}" | grep -o '=[^ ]*' | tr -d = | tr '\n' ' ')${peer:+; delivery ${peer[0]#--}}"
}

up() {
  local role="$1" port; port="$(port_of "$role")"
  [[ -x "$VM/bin/firecracker" && -f "$VM/vmlinux" ]] || die "run scripts/vm/setup.sh first"
  local kind; kind="$(kind_of "$role")"
  [[ -f "$VM/rootfs-$kind.ext4" ]] || die "run scripts/vm/build-rootfs.sh $kind first"
  alive "vm-$role" && { log "already running"; return; }
  alive "$role" && die "host $role service is running on :$port; stop it first"
  [[ -f "$VM/$role/data.ext4" ]] || adopt "$role"
  [[ -f "$ROOT/datadirs/$role.IN_VM" ]] || die "data disk exists but datadirs/$role.IN_VM is missing; refusing"
  mkdir -p "$RUN/$role" "$LOGS" "$PIDS"; chmod 0700 "$RUN"
  seeds_disk "$role"
  local uds="$RUN/$role/v.sock" cid; case "$role" in payer) cid=3 ;; recipient) cid=4 ;; wdk) cid=5 ;; esac
  rm -f "$uds" "$uds"_*
  cat > "$RUN/$role/vm.json" <<JSON
{
  "boot-source": { "kernel_image_path": "$VM/vmlinux",
    "boot_args": "console=ttyS0 reboot=k panic=1 pci=off ro init=/sbin/vm-init pe.role=$role" },
  "drives": [
    { "drive_id": "rootfs", "path_on_host": "$VM/rootfs-$kind.ext4", "is_root_device": true, "is_read_only": true },
    { "drive_id": "seeds", "path_on_host": "$RUN/$role/seeds.ext4", "is_root_device": false, "is_read_only": true },
    { "drive_id": "data", "path_on_host": "$VM/$role/data.ext4", "is_root_device": false, "is_read_only": false }
  ],
  "machine-config": { "vcpu_count": 2, "mem_size_mib": 768 },
  "vsock": { "guest_cid": $cid, "uds_path": "$uds" }
}
JSON
  : > "$LOGS/vm-$role.log"
  rm -f "$RUN/$role/api.sock"
  setsid "$VM/bin/firecracker" --api-sock "$RUN/$role/api.sock" --config-file "$RUN/$role/vm.json" --level Warning \
    >> "$LOGS/vm-$role.log" 2>&1 < /dev/null &
  echo $! > "$PIDS/vm-$role.pid"
  for _ in $(seq 1 50); do [[ -S "$uds" ]] && break; sleep 0.1; done
  bridge "$role"
  for i in $(seq 1 120); do
    health "$port" && break
    alive "vm-$role" || { tail -20 "$LOGS/vm-$role.log"; down "$role" >/dev/null; die "VM exited during boot"; }
    sleep 1
  done
  health "$port" || { tail -20 "$LOGS/vm-$role.log"; down "$role" >/dev/null; die "no /health after 120 s"; }
  rm -f "$RUN/$role/seeds.ext4"   # Firecracker holds it open; no seed file stays on the host besides secrets/
  log "up in ${i}s: VM pid $(cat "$PIDS/vm-$role.pid"), API on 127.0.0.1:$port via vsock, no network card"
  if [[ $role == recipient ]] && alive vm-payer; then ROLE=payer bridge payer; fi   # delivery becomes VM -> VM
}

down() {   # clean: Ctrl-Alt-Del -> init stops the wallet, syncs, unmounts; hard kill only after 30 s
  local role="$1" how="clean"
  if alive "vm-$role" && [[ -S "$RUN/$role/api.sock" ]]; then
    curl -s --max-time 5 --unix-socket "$RUN/$role/api.sock" -X PUT http://localhost/actions \
      -H 'Content-Type: application/json' -d '{"action_type": "SendCtrlAltDel"}' >/dev/null || true
    for _ in $(seq 1 150); do alive "vm-$role" || break; sleep 0.2; done
  fi
  if alive "vm-$role"; then how="HARD KILL (no clean exit)"; kill -KILL "$(cat "$PIDS/vm-$role.pid")" 2>/dev/null || true; fi
  if alive "vm-$role-bridge"; then kill -TERM "$(cat "$PIDS/vm-$role-bridge.pid")" 2>/dev/null || true; fi
  rm -f "$PIDS/vm-$role.pid" "$PIDS/vm-$role-bridge.pid" "$RUN/$role/seeds.ext4" "$RUN/$role/v.sock" "$RUN/$role/v.sock"_* "$RUN/$role/api.sock"
  log "down ($how)"
}

return_home() {   # copy the VM's wallet data back to the host and hand ownership back
  local role="$1" img="$VM/$1/data.ext4" out="$VM/build/return-$1"
  alive "vm-$role" && die "VM is running; run: scripts/vm/boot.sh down $role"
  [[ -f "$img" ]] || die "no data disk for $role"
  e2fsck -p "$img" >/dev/null || [[ $? -le 1 ]] || die "data disk check failed; not copying"
  rm -rf "$out"; mkdir -p "$out"
  debugfs -R "rdump / $out" "$img" >/dev/null 2>&1
  local data; data="$(data_of "$role")"
  [[ "$(cat "$out/OWNER")" == "vm:$role" && -d "$out/datadirs/$data" ]] || die "data disk content unexpected"
  local keep; keep="$ROOT/datadirs/$data.before-vm-$(date +%Y%m%d-%H%M%S)"
  mv "$ROOT/datadirs/$data" "$keep"
  cp -a "$out/datadirs/$data" "$ROOT/datadirs/$data"
  cp -a "$out/datadirs/journal/$role".sqlite3* "$ROOT/datadirs/journal/" 2>/dev/null || true
  [[ -d "$out/rgb-oob/$role" ]] && { rm -rf "$ROOT/.rgb-oob/$role"; cp -a "$out/rgb-oob/$role" "$ROOT/.rgb-oob/"; }
  mv "$img" "$img.returned-$(date +%Y%m%d-%H%M%S)"
  rm -rf "$out" "$ROOT/datadirs/$role.IN_VM"
  log "host owns the wallet again; stale pre-VM copy kept at $keep"
}

case "$CMD" in
  up) up "$ROLE" ;;
  down) down "$ROLE" ;;
  return) return_home "$ROLE" ;;
  rebridge) alive "vm-$ROLE" || die "VM not running"; bridge "$ROLE" ;;
  status)
    for r in payer recipient wdk; do
      if alive "vm-$r"; then s="VM running (pid $(cat "$PIDS/vm-$r.pid"))"; elif [[ -f "$ROOT/datadirs/$r.IN_VM" ]]; then s="VM owns data, stopped"; else s="host"; fi
      printf '%-10s %s\n' "$r" "$s"
    done ;;
  *) die "usage: boot.sh up|down|return|rebridge <payer|recipient|wdk> | status" ;;
esac
