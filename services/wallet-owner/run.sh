#!/usr/bin/env bash
# Start one wallet-owner process: ./services/wallet-owner/run.sh payer|recipient
# payer listens on 8711 and delivers consignments to the recipient on 8712.
set -euo pipefail

ROLE="${1:?usage: run.sh payer|recipient}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
case "$ROLE" in
  payer) PORT=8711; export PEER_URL="${PEER_URL:-http://127.0.0.1:8712}" ;;
  recipient) PORT=8712 ;;
  *) echo "role must be payer or recipient" >&2; exit 2 ;;
esac

if [[ -f "$ROOT/datadirs/$ROLE.IN_VM" ]]; then   # keys in a box: never two live copies of one wallet
  echo "refusing: the $ROLE wallet lives in its micro-VM now ($(cat "$ROOT/datadirs/$ROLE.IN_VM"))" >&2
  exit 3
fi

set -a
# shellcheck disable=SC1090
. "$ROOT/.env.$ROLE"
set +a
export BILL_ROOT="${BILL_ROOT:-$ROOT/../bill}"
export POWEREDGE_RGB_PROXY="${POWEREDGE_RGB_PROXY:-rpcs://proxy.kaleidoswap.com/json-rpc}"   # allowed RGB proxy (KaleidoSwap, Mutinynet)
export POWEREDGE_INVOICE_TRANSPORT="${POWEREDGE_INVOICE_TRANSPORT:-proxy}"   # crew invoices name the proxy: any RGB wallet can pay or receive
cd "$HERE"
exec "$HERE/.venv/bin/python" -m poweredge_wallet --port "$PORT"
