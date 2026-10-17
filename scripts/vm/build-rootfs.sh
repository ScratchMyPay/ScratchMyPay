#!/usr/bin/env bash
# Keys in a box, step 2: the wallet VMs' read-only root filesystem -> .vm/rootfs-wallet.ext4
# Code comes from git, not the working tree: wallet-owner at HEAD, Bill at its tested pin, so the image
# holds exactly the committed code and none of the working trees' secrets/ or wallet-data/.
# No root needed: docker export + mkfs.ext4 -d.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VM="$ROOT/.vm"; BUILD="$VM/build"
BILL_ROOT="${BILL_ROOT:-$ROOT/../bill}"; BILL_PIN="${BILL_PIN:-1986826}"
IMAGE="scratchmypay-wallet-vm"
log() { printf '[vm-rootfs] %s\n' "$*"; }

rm -rf "$BUILD"; mkdir -p "$BUILD/ctx/opt/poweredge/scripts/vm" "$BUILD/ctx/opt/bill"
git -C "$ROOT" archive HEAD services/wallet-owner | tar -x -C "$BUILD/ctx/opt/poweredge" --exclude='services/wallet-owner/tests'
git -C "$BILL_ROOT" archive "$BILL_PIN" web dock | tar -x -C "$BUILD/ctx/opt/bill"
cp "$ROOT/scripts/vm/vsock-bridge.py" "$BUILD/ctx/opt/poweredge/scripts/vm/"
cp "$ROOT/scripts/vm/vm-init" "$BUILD/ctx/vm-init"
printf 'scratchmypay %s\nbill %s\n' "$(git -C "$ROOT" rev-parse --short HEAD)" "$BILL_PIN" > "$BUILD/ctx/opt/poweredge/BUILD"
log "code: $(tr '\n' ' ' < "$BUILD/ctx/opt/poweredge/BUILD")"

docker build -q -t "$IMAGE" -f "$ROOT/scripts/vm/Dockerfile.wallet" "$BUILD/ctx" >/dev/null
cid="$(docker create "$IMAGE")"
trap 'docker rm -f "$cid" >/dev/null 2>&1 || true' EXIT
mkdir -p "$BUILD/rootfs"
docker export "$cid" | tar -x -C "$BUILD/rootfs" --exclude='dev/*' --no-same-owner 2>/dev/null || true
[[ -x "$BUILD/rootfs/sbin/vm-init" && -x "$BUILD/rootfs/usr/local/bin/python3" ]] || { log "export incomplete"; exit 1; }
rm -f "$VM/rootfs-wallet.ext4"
mkfs.ext4 -q -F -L wallet-root -d "$BUILD/rootfs" "$VM/rootfs-wallet.ext4" 1G
rm -rf "$BUILD/rootfs"
log "image $VM/rootfs-wallet.ext4 ($(du -h "$VM/rootfs-wallet.ext4" | cut -f1) used of 1G)"
