# Wallet-owner service

The only PowerEdge process that holds RGB keys. One process per wallet: **payer** (organizer, port 8711) and **recipient** (crew member, port 8712). Loopback only, bearer token, Mutinynet only.

It wraps the prior-work RGB Bill adapter (`EdwinKestler/bill@be3d237`, `web/rgb/`) unchanged and adds what a payout product needs:

- **Durable journal** (`datadirs/journal/<role>.sqlite3`, WAL, `synchronous=FULL`): `PLANNED → APPROVED → SUBMITTING → BROADCAST → DELIVERED → SETTLED`. `SUBMITTING` is committed *before* the send. After a crash it becomes `UNKNOWN_SUBMISSION` and is reconciled from wallet transfers; it is never resent.
- **Approval bound to the flow**: approve needs the exact plan digest and the n2link flow revision the plan was made under; a redeploy voids it.
- **Out-of-band delivery**: after the send, the payer posts the consignment to the recipient service, which imports and validates it.
- **Stable error codes** (`not_approved`, `flow_changed`, `unknown_submission`, `wallet_refused`, …) that the guided app turns into plain sentences.

## Run

```bash
# once
python3 -m venv services/wallet-owner/.venv
services/wallet-owner/.venv/bin/pip install -r services/wallet-owner/requirements.txt
BILL_ROOT=../bill services/wallet-owner/.venv/bin/python services/wallet-owner/scripts/provision.py --role payer
BILL_ROOT=../bill services/wallet-owner/.venv/bin/python services/wallet-owner/scripts/provision.py --role recipient
# fund both printed addresses from the Mutinynet faucet, then on the payer:
#   create colored UTXOs (Bill: scripts.rgb_wallet create-utxos) and
#   services/wallet-owner/scripts/issue_asset.py   (TUSDT, 6 decimals)

# every time (recipient first; the public indexer rate-limits back-to-back syncs)
services/wallet-owner/run.sh recipient &
sleep 30
services/wallet-owner/run.sh payer &
```

Secrets (`secrets/*.mnemonic`, `secrets/wallet-api.token`, `.env.payer`, `.env.recipient`, `datadirs/`) are created locally and git-ignored.

## Crash demo

```bash
POWEREDGE_CRASH_AT=after_send services/wallet-owner/run.sh payer   # dies right after broadcasting
services/wallet-owner/run.sh payer                                 # restart: execute returns the journal row, never resends
```

## Test

```bash
cd services/wallet-owner && BILL_ROOT=../../../bill .venv/bin/python -m pytest -q tests
```
