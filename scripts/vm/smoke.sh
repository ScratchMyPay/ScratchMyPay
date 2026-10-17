#!/usr/bin/env bash
# Keys in a box, smoke test: boot one VM with no wallet and no secrets, ask it over vsock for the Mutinynet
# tip (fetched through the host pinhole with real TLS), confirm it has no network card, destroy it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VM="$ROOT/.vm"; RUN="/tmp/scratchmypay-vm/smoke"; PORT="${SMOKE_PORT:-18711}"
mkdir -p "$RUN"; chmod 0700 "$RUN" /tmp/scratchmypay-vm
rm -f "$RUN/v.sock" "$RUN"/v.sock_*
cat > "$RUN/vm.json" <<JSON
{ "boot-source": { "kernel_image_path": "$VM/vmlinux",
    "boot_args": "console=ttyS0 reboot=k panic=1 pci=off ro init=/sbin/vm-init pe.role=smoke" },
  "drives": [ { "drive_id": "rootfs", "path_on_host": "$VM/rootfs-wallet.ext4", "is_root_device": true, "is_read_only": true } ],
  "machine-config": { "vcpu_count": 1, "mem_size_mib": 256 },
  "vsock": { "guest_cid": 9, "uds_path": "$RUN/v.sock" } }
JSON
t0=$(date +%s.%N)
"$VM/bin/firecracker" --no-api --config-file "$RUN/vm.json" --level Warning > "$RUN/console.log" 2>&1 < /dev/null &
FC=$!
for _ in $(seq 1 50); do [[ -S "$RUN/v.sock" ]] && break; sleep 0.1; done
python3 "$ROOT/scripts/vm/vsock-bridge.py" --expose "127.0.0.1:$PORT=$RUN/v.sock:8711" \
  --pinhole "$RUN/v.sock:10001=mutinynet.com:443" > "$RUN/bridge.log" 2>&1 &
BR=$!
trap 'kill $BR $FC 2>/dev/null; wait $FC 2>/dev/null; rm -f "$RUN/v.sock" "$RUN"/v.sock_*' EXIT
out=""
for _ in $(seq 1 60); do out="$(curl -s --max-time 20 "http://127.0.0.1:$PORT/" || true)"; [[ "$out" == *'"ok"'* ]] && break; sleep 0.5; done
dt=$(python3 -c "print(round($(date +%s.%N) - $t0, 1))")
echo "$out"
if [[ "$out" == *'"ok": true'* && "$out" == *'"network_cards": ["lo"]'* ]]; then echo "SMOKE PASS in ${dt}s (boot + vsock + pinhole TLS, only loopback inside)"
else echo "SMOKE FAIL after ${dt}s"; echo "--- console"; tail -25 "$RUN/console.log"; echo "--- bridge"; tail -5 "$RUN/bridge.log"; exit 1; fi
