#!/usr/bin/env bash
# Keys in a box, step 2: read-only root filesystems -> .vm/rootfs-wallet.ext4 (organizer, crew)
#   scripts/vm/build-rootfs.sh [wallet|treasury]      treasury -> .vm/rootfs-treasury.ext4 (WDK, Node)
# Code comes from git, not the working tree: wallet-owner at HEAD, Bill at its tested pin, so the image
# holds exactly the committed code and none of the working trees' secrets/ or wallet-data/.
# No root needed: docker export + mkfs.ext4 -d.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VM="$ROOT/.vm"; BUILD="$VM/build"
BILL_ROOT="${BILL_ROOT:-$ROOT/../bill}"; BILL_PIN="${BILL_PIN:-1986826}"
KIND="${1:-wallet}"; [[ "$KIND" == wallet || "$KIND" == treasury ]] || { echo "usage: build-rootfs.sh [wallet|treasury]"; exit 2; }
IMAGE="scratchmypay-$KIND-vm"
log() { printf '[vm-rootfs] %s\n' "$*"; }

rm -rf "$BUILD"; mkdir -p "$BUILD/ctx/opt/poweredge/scripts/vm" "$BUILD/ctx/opt/bill"
if [[ "$KIND" == wallet ]]; then
  git -C "$ROOT" archive HEAD services/wallet-owner | tar -x -C "$BUILD/ctx/opt/poweredge" --exclude='services/wallet-owner/tests'
  git -C "$BILL_ROOT" archive "$BILL_PIN" web dock | tar -x -C "$BUILD/ctx/opt/bill"
else
  git -C "$ROOT" archive HEAD services/wdk-btc | tar -x -C "$BUILD/ctx/opt/poweredge" --exclude='*.test.mjs'
  rmdir "$BUILD/ctx/opt/bill"
fi
cp "$ROOT/scripts/vm/vsock-bridge.py" "$BUILD/ctx/opt/poweredge/scripts/vm/"
cp "$ROOT/scripts/vm/vm-init" "$BUILD/ctx/vm-init"
printf 'scratchmypay %s\nbill %s\n' "$(git -C "$ROOT" rev-parse --short HEAD)" "$BILL_PIN" > "$BUILD/ctx/opt/poweredge/BUILD"
log "code: $(tr '\n' ' ' < "$BUILD/ctx/opt/poweredge/BUILD")"

docker build -q -t "$IMAGE" -f "$ROOT/scripts/vm/Dockerfile.$KIND" "$BUILD/ctx" >/dev/null
cid="$(docker create "$IMAGE")"
trap 'docker rm -f "$cid" >/dev/null 2>&1 || true' EXIT
mkdir -p "$BUILD/rootfs"
docker export "$cid" | tar -x -C "$BUILD/rootfs" --exclude='dev/*' --no-same-owner 2>/dev/null || true
[[ -x "$BUILD/rootfs/sbin/vm-init" && ( -x "$BUILD/rootfs/usr/local/bin/python3" || -x "$BUILD/rootfs/usr/bin/python3" ) ]] || { log "export incomplete"; exit 1; }
OUT="$VM/rootfs-$KIND.ext4"
rm -f "$OUT"
mkfs.ext4 -q -F -L "$KIND-root" -d "$BUILD/rootfs" "$OUT" 1G
rm -rf "$BUILD/rootfs"
log "image $OUT ($(du -h "$OUT" | cut -f1) used of 1G)"
