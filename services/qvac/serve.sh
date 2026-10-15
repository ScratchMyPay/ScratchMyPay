#!/usr/bin/env bash
# Start the local QVAC OpenAI-compatible server for PowerEdge.
# Loopback only, bearer-token protected. No cloud fallback exists anywhere in PowerEdge.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
PORT="${QVAC_PORT:-11435}"
KEY_FILE="${QVAC_API_KEY_FILE:-$ROOT/secrets/qvac.api-key}"

if [[ ! -f "$KEY_FILE" ]]; then
  mkdir -p "$(dirname "$KEY_FILE")"
  chmod 700 "$(dirname "$KEY_FILE")"
  umask 077
  openssl rand -hex 32 > "$KEY_FILE"
  echo "Created local QVAC API key at $KEY_FILE (git-ignored)."
fi

cd "$HERE"
exec npx --no-install qvac serve --openai --no-default \
  --config "$HERE/qvac.config.json" \
  --host 127.0.0.1 --port "$PORT" \
  --api-key-file "$KEY_FILE" \
  --model poweredge-intent "$@"
