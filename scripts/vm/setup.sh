#!/usr/bin/env bash
# Keys in a box, step 1: pinned Firecracker + guest kernel into .vm/ (git-ignored).
#   scripts/vm/setup.sh            download if missing, verify, print versions
#   scripts/vm/setup.sh --prefetch same (for a laptop going to the venue)
# Firecracker: SHA-256 from the GitHub release. Kernel: Firecracker's CI kernel; its SHA-256 is
# pinned in scripts/vm/pins.txt on first download and every later download must match it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VM="$ROOT/.vm"; PINS="$ROOT/scripts/vm/pins.txt"
FC_VERSION="v1.17.0"
KERNEL_KEY="firecracker-ci/v1.15/x86_64/vmlinux-6.1.155"
log() { printf '[vm-setup] %s\n' "$*"; }
die() { printf '[vm-setup] ERROR: %s\n' "$*" >&2; exit 1; }

[[ -r /dev/kvm && -w /dev/kvm ]] || die "/dev/kvm is not read-write for $(id -un) (add to group kvm or a uaccess ACL)"
mkdir -p "$VM/bin" "$VM/dl"

pin() {   # pin <name> <file>: record the SHA on first sight, refuse a different one later
  local name="$1" sha; sha="$(sha256sum "$2" | cut -d' ' -f1)"
  touch "$PINS"
  local want; want="$(awk -v n="$name" '$1 == n {print $2}' "$PINS")"
  if [[ -z "$want" ]]; then printf '%s %s\n' "$name" "$sha" >> "$PINS"; log "pinned $name $sha"
  elif [[ "$want" != "$sha" ]]; then die "$name SHA-256 $sha does not match pin $want"; fi
}

if [[ ! -x "$VM/bin/firecracker" ]]; then
  tgz="firecracker-$FC_VERSION-x86_64.tgz"
  base="https://github.com/firecracker-microvm/firecracker/releases/download/$FC_VERSION"
  log "downloading Firecracker $FC_VERSION"
  curl -fsSL -o "$VM/dl/$tgz" "$base/$tgz"
  curl -fsSL -o "$VM/dl/$tgz.sha256.txt" "$base/$tgz.sha256.txt"
  (cd "$VM/dl" && sha256sum -c "$tgz.sha256.txt") || die "Firecracker release SHA-256 mismatch"
  pin "$tgz" "$VM/dl/$tgz"
  tar -xzf "$VM/dl/$tgz" -C "$VM/dl"
  install -m 0755 "$VM/dl/release-$FC_VERSION-x86_64/firecracker-$FC_VERSION-x86_64" "$VM/bin/firecracker"
fi

if [[ ! -f "$VM/vmlinux" ]]; then
  log "downloading guest kernel $KERNEL_KEY"
  curl -fsSL -o "$VM/dl/vmlinux" "https://s3.amazonaws.com/spec.ccfc.min/$KERNEL_KEY"
  pin "$(basename "$KERNEL_KEY")" "$VM/dl/vmlinux"
  mv "$VM/dl/vmlinux" "$VM/vmlinux"
fi

log "$("$VM/bin/firecracker" --version | head -1)"
log "kernel $(basename "$KERNEL_KEY") ($(du -h "$VM/vmlinux" | cut -f1))"
log "ready: $VM"
