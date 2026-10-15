#!/usr/bin/env bash
# Start n2link with the PowerEdge RGB nodes and guided app.
#   App:    http://127.0.0.1:1888/poweredge-app
#   Editor: http://127.0.0.1:1888/
# Needs: QVAC (services/qvac/serve.sh) and both wallet services (services/wallet-owner/run.sh).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOME_DIR="$ROOT/datadirs/n2link"
BIN="$ROOT/n2link/target/debug/n2linkd"
mkdir -p "$HOME_DIR"
[[ -f "$HOME_DIR/flows.json" ]] || echo "[]" > "$HOME_DIR/flows.json"
if [[ ! -x "$BIN" ]]; then
  (cd "$ROOT/n2link" && cargo build --features nodes_rgb)
fi
if [[ ! -f "$ROOT/n2link/target/ui_static/red/red.min.js" ]]; then
  echo "WARNING: the flow editor is not built; http://127.0.0.1:1888/ will be blank." >&2
  echo "         Run scripts/setup-editor.sh (the guided app at /poweredge-app still works)." >&2
fi

export POWEREDGE_TEMPLATES_DIR="$ROOT/templates"
export POWEREDGE_WALLET_TOKEN_FILE="$ROOT/secrets/wallet-api.token"
export POWEREDGE_QVAC_KEY_FILE="$ROOT/secrets/qvac.api-key"
export POWEREDGE_QVAC_URL="${POWEREDGE_QVAC_URL:-http://127.0.0.1:11435/v1}"
export POWEREDGE_QVAC_MODEL="${POWEREDGE_QVAC_MODEL:-poweredge-intent}"
export POWEREDGE_PAYER_URL="${POWEREDGE_PAYER_URL:-http://127.0.0.1:8711}"
export POWEREDGE_RECIPIENT_URL="${POWEREDGE_RECIPIENT_URL:-http://127.0.0.1:8712}"

exec "$BIN" run --home "$HOME_DIR" --bind 127.0.0.1:1888 "$HOME_DIR/flows.json"
