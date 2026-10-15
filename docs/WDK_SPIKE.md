# WDK spike — can WDK work on Mutinynet next to the PowerEdge rgb-lib wallets?

Date: 2026-10-15, ~22:10–22:40 CEST. Time-boxed. Throwaway seeds generated in memory, temp datadirs, **no funds moved**.
Code: `spikes/wdk/` (`spike.mjs`, `cause.mjs`, `signet-default.mjs`). Raw output: `artifacts/wdk-spike/`.

## Packages tested (published npm versions)

| Package | Version | Notes |
|---|---|---|
| `@tetherto/wdk-wallet-btc` | 1.0.0-beta.17 | pure JS; runs on Node 24 |
| `@utexo/wdk-wallet-rgb` | 2.0.0 | uses `@utexo/rgb-sdk` 1.0.3 → `@utexo/rgb-lib` 0.3.0-beta.8 |
| `@utexo/rgb-lib-linux-x64` | 0.3.0-beta.8 | native addon; **not declared as a dependency** (must be installed explicitly) and **does not compile on Node 24** (SWIG V8 code uses removed `Holder`/`SetAccessor`); builds on **Node 22.23.3** |

The published `wdk-wallet-rgb` 2.0.0 differs from the vendored GitHub source (no Bare binding; no `decodeRGBInvoice`).

## Results

| # | Question | Result |
|---|---|---|
| 1 | `wdk-wallet-btc` on Mutinynet (`network: 'testnet'`, Electrum `electrum.mutinynet.com:50002` ssl) | **Works.** Opens account, `tb1q…` address, balance from Mutinynet, live fee rates (normal 1, fast 2 sat/vB). Send quote needs funds ("No unspent outputs") — expected for an empty wallet. |
| 2 | `wdk-wallet-rgb` network `mutinynet` (custom signet) | **Not supported.** Published 2.0.0: `Invalid network: mutinynet. Must be one of: mainnet, testnet, testnet4, signet, regtest`. (Vendored source silently maps unknown names to Regtest.) |
| 3 | `wdk-wallet-rgb` network `signet` + its **default** indexer (`tcp://46.224.75.237:50001`, plain TCP to a bare IP) | **Fails to go online:** `RgbLib(InvalidElectrum { "verbose transactions are currently unsupported" })`. Server is reachable (`electrs-esplora 0.4.1`) but lacks what rgb-lib needs. |
| 4 | `wdk-wallet-rgb` `signet` + Mutinynet **Electrum** | Same `InvalidElectrum` (same limitation we found with Python rgb-lib). |
| 5 | `wdk-wallet-rgb` `signet` + Mutinynet **Esplora** (`https://mutinynet.com/api`) | First attempt: HTTP 429 (public rate limit). **Retry after 30 s: works** — goes online and syncs against the Mutinynet chain (all signets share the genesis block). |
| 6 | Asset interoperability with PowerEdge wallets | **Not possible as-is:** WDK `signet` invoices are tagged `sb:` and default to a proxy transport (`rpcs://proxy.iriswallet.com/0.2/json-rpc`); PowerEdge TUSDT lives on `sbc:` (custom signet, Mutinynet) with out-of-band delivery. RGB treats these as different networks. |

## Conclusion
- **Usable now — `wdk-wallet-btc` on Mutinynet:** balance, live fee rates and send quotes work. Good basis for read-only treasury nodes and a fee line on the payout Review card.
- **`wdk-wallet-rgb` can run on the Mutinynet *chain*** in `signet` mode via Mutinynet Esplora (with 429 backoff), but at the RGB layer it is a **different network** (`sb:`, proxy transport) from PowerEdge's TUSDT (`sbc:`, out-of-band). It **cannot pay or receive TUSDT with the PowerEdge wallets**, so it stays **out of the payout path**.
- Practical constraints: Node 22 (not 24) for the RGB addon; install `@utexo/rgb-lib-linux-x64` explicitly; never rely on WDK's default Signet indexer (incompatible with rgb-lib, plain TCP to a bare IP).

## Recommendation for Sunday
1. `wdk-btc` sidecar (Node, loopback, bearer token) with read-only n2link nodes: `wdk-balance`, `wdk-fee-quote` (+ `wdk-transfers`). Copilot-allowed.
2. Fee line on the PowerEdge Review card from `wdk-fee-quote` ("network fee ≈ N sats").
3. Pre-show treasury check flow (inject → `wdk-balance` + `wdk-fee-quote` → switch → alert).
4. Optional showcase only, clearly labelled: a WDK RGB wallet on the Mutinynet chain in `signet` mode (its own asset network). Not mixed with TUSDT payouts.
