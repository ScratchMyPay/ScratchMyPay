# WDK treasury service (Tether WDK, Mutinynet)

Read-only treasury for PowerEdge built on [`@tetherto/wdk-wallet-btc`](https://github.com/tetherto/wdk-wallet-btc) 1.0.0-beta.17.
Loopback only (`127.0.0.1:8713`), same bearer token as the wallet services, Mutinynet only. **It has no endpoint that sends.**

| Endpoint | Returns |
|---|---|
| `GET /health` | engine + network |
| `GET /balance` | treasury address and balance (sats) |
| `GET /fees?count=N` | live WDK fee rates × measured PowerEdge payout size (197 vB + 1,000 sat witness) for N payouts |
| `GET /check?count=N&speed=normal` | can the treasury cover N payouts? (`coverage.enough`, `payoutsCovered`, `shortSat`) |
| `GET /transfers?limit=N` | recent treasury transfers |

The payout size was measured from the two live PowerEdge payouts on Mutinynet (txids `ea4ce78b…`, `00d20ebd…`: 197 vB each).

n2link nodes using it (feature `nodes_rgb`, palette category **WDK**): `wdk-wallet-ref` (config), `wdk-balance`, `wdk-fee-quote`, `wdk-transfers`. The PowerEdge app shows the WDK network cost on the Review and Approve screens.

## Run

```bash
cd services/wdk-btc && npm install && cd ../..
node services/wdk-btc/server.mjs          # creates secrets/wdk-treasury.mnemonic (0600, git-ignored) on first run
```

Fund the printed treasury address from the Mutinynet faucet to see `coverage.enough` turn true.

## Test

```bash
cd services/wdk-btc && node --test
```

Notes from the WDK spike (`docs/WDK_SPIKE.md`): Mutinynet works with `network: 'testnet'` + Electrum `electrum.mutinynet.com:50002` (ssl). `@utexo/wdk-wallet-rgb` is not used: it cannot join the Mutinynet (`sbc:`) TUSDT payouts.
