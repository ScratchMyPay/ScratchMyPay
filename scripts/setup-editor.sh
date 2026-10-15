#!/usr/bin/env bash
# Fetch and build the n2link flow editor (Node-RED editor client).
#
# The editor is the Node-RED web client, pinned as a git submodule at the same commit
# upstream n2link uses (Node-RED 5.0.7, d9644c41). n2link's build.rs builds it with npm and
# copies it into n2link/target/ui_static. Without it, http://127.0.0.1:1888/ is a blank page
# (the guided app at /poweredge-app does not need it).
#
# Needs: git, Node.js + npm (any current LTS), Rust.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PIN="d9644c41beb0d90d1cbf9375646e4259551182af"
SUB="n2link/3rd-party/node-red"
cd "$ROOT"

echo "==> Fetching Node-RED editor source (pinned ${PIN:0:9})"
git submodule update --init --depth 1 "$SUB" || true
if [[ "$(git -C "$SUB" rev-parse HEAD 2>/dev/null)" != "$PIN" ]]; then
  git -C "$SUB" fetch --depth 1 origin "$PIN"
  git -C "$SUB" checkout -q "$PIN"
fi
echo "    $(git -C "$SUB" log --oneline -1)"

command -v npm >/dev/null || { echo "npm is required to build the editor (install Node.js LTS)" >&2; exit 1; }

echo "==> Building n2link with the PowerEdge nodes (build.rs also builds the editor; first run takes a few minutes)"
(cd n2link && cargo build --features nodes_rgb)

if [[ -f n2link/target/ui_static/red/red.min.js && -f n2link/target/ui_static/vendor/vendor.js ]]; then
  echo "==> Editor ready: $(cat n2link/target/ui_static/.editor-version 2>/dev/null || echo built)"
else
  echo "Editor files are missing from n2link/target/ui_static; see the build output above." >&2
  exit 1
fi
