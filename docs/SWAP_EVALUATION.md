# Swaps between KaleidoSwap and WDK: evaluation

Date: 2026-10-17, 23:15–23:50 CEST. Research only: no code or design change.

Question: how hard is it to swap between KaleidoSwap (RGB USDT, Lightning) and Tether WDK (BTC, USDT), in either direction, with the existing tooling?

## Facts

- **KaleidoSwap's test maker runs on Mutinynet**, the network ScratchMyPay already uses. Queried live (`GET https://api.signet.kaleidoswap.com/api/v1/market/assets` and `/market/pairs`, 23:20): `network: signetcustom`; pairs BTC/USDT, BTC/XAUT, XAUT/USDT, all active.
- **BTC/USDT supports on-chain routes, not only Lightning:** `BTC_L1→RGB_L1`, `BTC_L1→RGB_LN`, `BTC_LN→RGB_L1`, `BTC_LN→RGB_LN` and the four reverse routes. Limits from the API: USDT 0.5–1,000 (precision 6); BTC 100,000–1,000,000,000 units (precision 11). BTC also lists `BTC_SPARK`, `BTC_ARKADE`, `BTC_LIQUID` and `BTC_CASHU` as supported layers, but the BTC/USDT pair only offers `BTC_L1` and `BTC_LN`.
- **USDT exists only on RGB layers** (`RGB_L1`, `RGB_LN`): there is no route to USDT on Tron or EVM.
- **Lightning swaps need your own RGB Lightning Node with a channel** (kaleido-cli: Docker RLN; it creates a Mutinynet node by default; flow: quote, then swap order or atomic init → whitelist → execute). The CLI is beta, intended for testnet/signet only.
- **WDK Lightning (`@tetherto/wdk-wallet-spark`)** supports only Spark's MAINNET, SIGNET and REGTEST, not Mutinynet. On testnet it cannot pay a KaleidoSwap Mutinynet invoice.
- **`@utexo/wdk-rgb-lightning`** (community, 0.1.0-beta.15) runs an RGB Lightning node behind a WDK account, but its atomic-swap methods are native-binding-only, outside the WDK account interface. Mutinynet support is not documented.

## Options

Difficulty: 1 easy … 5 hard.

| # | Option | Difficulty | Time | Mutinynet today | Main risks |
|---|---|---|---|---|---|
| 1 | Live swap quote, read-only (`kaleido-quote` node: BTC↔USDT price, limits, routes on the dashboard and the treasury check) | 1 | 1–1.5 h | Yes | One more outbound pinhole |
| 2 | RGB USDT → BTC into the WDK treasury (`RGB_L1→BTC_L1`): organizer pays the maker's RGB invoice through the exactly-once path with approval; WDK only receives and stays read-only | 2.5 | 3–5 h | Yes | Order flow to verify (~15 min); an on-chain order trusts the maker to send (not atomic); maker liquidity |
| 3 | WDK treasury BTC → RGB USDT to the organizer (`BTC_L1→RGB_L1`): treasury pays the maker's deposit address; organizer receives with its RGB invoice (proxy flow already proven) | 3 | 4–6 h | Yes | Turns on WDK sending (design change: approval gate, pinhole from the treasury VM to the maker); refund path; maker-trusted |
| 4 | Atomic Lightning swap with our own RGB Lightning Node (kaleido-cli, channel from KaleidoSwap's LSP), `BTC_LN↔RGB_LN` | 4 | 1–2 days | In principle | New hot node outside the key boxes; channel purchase; liquidity; alpha software |
| 5 | Same, WDK-native via `@utexo/wdk-rgb-lightning` | 4.5 | 2–3+ days | Unverified | Beta; swap methods outside WDK; network support unknown |
| 6 | WDK Spark (Lightning) ↔ KaleidoSwap | 3 on mainnet | 1–2 days | No (network mismatch) | Real money only |
| 7 | RGB USDT ↔ WDK USDT on Tron/EVM (USDT0) | 5 | Weeks, partner-dependent | No | No route or bridge |

## Recommendation

- **During the hackathon:** at most option 1 (Sunday morning, ~1 h): judges see a live KaleidoSwap quote next to the WDK treasury, with no new risk.
- **First real integration after the hackathon:** option 2. It reuses the exactly-once RGB send and keeps WDK read-only (no new spending key). Say plainly that an on-chain swap order trusts the maker, unlike an atomic Lightning swap.
- Option 3 only after a decision on WDK sending; options 4 and 5 are a separate project.

## Sources

- kaleido-cli: https://pypi.org/project/kaleido-cli/
- kaleido-sdk: https://pypi.org/project/kaleidoswap-sdk/
- WDK Spark API: https://docs.wallet.tether.io (wallet-spark API reference)
- WDK community module `@utexo/wdk-rgb-lightning`: WDK docs, community modules
- KaleidoSwap Mutinynet maker API, queried live 2026-10-17 23:20 CEST
