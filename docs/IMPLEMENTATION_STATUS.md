> Brand: **ScratchMyPay** (codename RGB2link PowerEdge until 2026-10-16).

# Implementation status (factual ledger)

Only what has been run and observed goes here. "Planned" lives in `IMPLEMENTATION_PLAN.md`.

## Environment (2026-10-15)

| Item | Value |
|---|---|
| Host | andorxps, Linux, 32 cores, 60 GB RAM, NVIDIA RTX 3090 Ti 24 GB |
| Rust | `rustc 1.99.0 (b940084d7 2026-09-28)` |
| Node | v24.15.0 |
| Python (wallet) | Bill venv: `rgb-lib==0.3.0b7`, `tetherto-qvac-sdk==0.21.0` |
| n2link | subtree `n2link/` = `78d7ba107ed3ade008a9fcc5ab192755c11b8299` (`0.4.0-rc.1`), commit `3454c90` |
| Bill (dependency) | `fork2checkbook/fork2checkbook@1986826` (formerly `EdwinKestler/bill`, declared `be3d237`; wallet-owner tests 29/29 against a clean checkout of `1986826`) |

## Ledger

| Date (CEST) | Item | Result | Evidence |
|---|---|---|---|
| 2026-10-15 18:16–18:18 | n2link baseline `cargo fmt --check` | pass (exit 0) | `/tmp/poweredge-baseline/fmt.log` |
| 2026-10-15 18:16–18:18 | n2link baseline `cargo test --workspace` (default features) | **416 passed, 0 failed**, 15 suites, exit 0 | `/tmp/poweredge-baseline/test.log` |
| 2026-10-15 | Repo public; `.gitignore` hardened; history scanned for secrets | clean (doc mentions only) | commit `939bf5b` |
| 2026-10-15 | n2link subtree tracked "credential" files reviewed | fixture/redacted values only, already public upstream | `n2link/adoption/*/fixtures/flows_cred.json` |
| 2026-10-15 | Mutinynet wallets provisioned (payer = Bill role A, recipient = role B) | created, mnemonics 0600 in git-ignored `secrets/`, env in git-ignored `.env.payer` / `.env.recipient` | `services/wallet-owner/scripts/provision.py` |
| 2026-10-15 | Payer funding address | `tb1pncw03gkwd65gydgq2pq499xq09nsdymhp6eara5cq9de8m9mjdzs3dd26s` | funded |
| 2026-10-15 | Recipient funding address | `tb1pglk2u0d64upreatyl6sz3sy7yny583l4qu40rh092gg0jhdkfngqyt6t4e` | funded |
| 2026-10-15 ~18:45 | Faucet funding confirmed on-chain (Esplora API) | payer 100,000 sats (UTXO `a971a954…:1`), recipient 20,000 sats (UTXO `44d86fd1…:1`), both confirmed | mutinynet.com/api |
| 2026-10-15 ~19:00 | rgb-lib wallet sync via Bill `rgb_wallet status` | payer: vanilla spendable 100,000, colored 0, no assets; recipient: vanilla spendable 20,000, colored 0, no assets | Esplora `https://mutinynet.com/api` |
| 2026-10-15 | **Finding:** Esplora returns HTTP 429 on back-to-back fresh-wallet syncs | transient (Cloudflare); succeeded after ~20–60 s backoff | see risk R-INDEXER |
| 2026-10-15 | **Finding:** Electrum `ssl://electrum.mutinynet.com:50002` rejected by rgb-lib | `InvalidIndexer: resolver returned invalid data` — not usable | Esplora only |
| 2026-10-15 | QVAC server up | `@qvac/cli 0.15.0`, `@qvac/sdk 0.21.0`, Qwen3-8B-Q4_K_M (5.03 GB), `127.0.0.1:11435`, bearer key | `services/qvac/` |
| 2026-10-15 | QVAC WorkflowIntent smoke (json_schema, temp 0) | 3/3 schema-valid; hero 9.5 s (first call, warm-up), injections 1.1 s; exec/mainnet/skip-approval requests refused in `rationale`, environment kept `mutinynet` | `artifacts/qvac-smoke/*.json` |
| 2026-10-15 ~19:00 | Payer colored UTXOs (`create-utxos`, typed confirmation) | 6 × 10,000 sats; txid `efd2c485fa4bf0148162098a722f7cc8ae000de956dc62a279098e3531b32921`, fee 739 sats, confirmed at block 3,508,146; payer now 60,000 colored / 39,261 vanilla | Bill `rgb_wallet create-utxos` |
| 2026-10-15 19:05 | **TUSDT issued** (NIA, offline issuance on payer, typed confirmation) | `rgb:I7olLl3y-c49Ha7C-v82SFEj-BU33dvn-4mGtCIL-MNRyWm8`, "PowerEdge Test USD", precision 6, supply 1,000,000 units (10^12 base units) | `artifacts/asset/TUSDT.json` |
| 2026-10-15 | **Finding:** rgb-lib 0.3.0b7 requires uppercase tickers and issues NIA offline (no `online` arg; protocol stub in `rgb_lib.py` differs from concrete `Wallet`) | asset ticker is `TUSDT`; compiler normalizes typed tickers to uppercase; always check signatures with `inspect.signature(rgb_lib.Wallet.<m>)` | `services/wallet-owner/scripts/issue_asset.py` |
| 2026-10-15 | `poweredge` crate (intent, registry, compiler, validator) | **20 tests passed, 0 failed**; `cargo clippy -D warnings` clean; `cargo fmt --check` clean | `n2link/crates/poweredge`, commit `51a2606`+ |
| 2026-10-15 | `rgb-payment-v1@1.0.0` pinned | digest `fe668c53ce71f152a83e370c9c4d038c9c26d465edaa24af307888952b362917`; `poweredge-template verify templates` ok | `templates/rgb-payment-v1/` |
| 2026-10-15 ~19:20 | wallet-owner service (FastAPI + SQLite journal over Bill's adapter) | **20 tests passed**: crash before/after send, ambiguous send → UNKNOWN → reconciled, 8-way concurrent execute = 1 send, flow change voids approval, balance drift refused before broadcast, roles, any-asset invoice, HTTP contract | `services/wallet-owner/` |
| 2026-10-15 ~19:25 | Live services on Mutinynet (payer :8711, recipient :8712, loopback, bearer token) | payer `/assets` → TUSDT spendable 1,000,000.000000; missing token → 401 | `/tmp/poweredge/wallet-*.log` |
| 2026-10-15 ~19:30 | **Finding:** rgb-lib `witness_receive` with an asset the recipient never held → `AssetNotFound` | first-time recipients invoice "any asset" (ADR-016); unhandled rgb-lib errors now map to `wallet_error` instead of HTTP 500 | `service.py` |
| 2026-10-15 ~19:35 | Live recipient invoice (any asset) | `rgb:~/~/~/sbc:wvout:BbT58Cr9-…?expiry=1792088784` (Mutinynet `sbc:`) | recipient service |
| 2026-10-15 ~19:40 | Live payer **plan only** (not approved, not executed) for 25 TUSDT against that invoice | `PLANNED`, plan `17d078af-…`, digest `60a04a05…`; Bill invoice rules + balance check passed; left to expire | payer journal |
| 2026-10-15 | `nodes_rgb` in n2link-core: `rgb-wallet-ref`, `rgb-invoice`, `rgb-plan`, `approval-gate`, `rgb-approve`, `rgb-send-once`, `rgb-refresh`, `rgb-receipt`, `rgb-status` + editor palette (`crates/web/rgb-editor/`) | compiles; `cargo clippy -D warnings --all-targets` clean on core/web/poweredge with `nodes_rgb` | ADR-007, ADR-018 |
| 2026-10-15 | `rgb-payment-v1` re-pinned after adding the approval endpoint, `templateDigest` and query-based status | digest `3eccc585999d06d4a8ed74bedfcdbeea475ef656296d5ba336b98775d2d7a890`; poweredge 20/20 | `templates/rgb-payment-v1/` |
| 2026-10-15 ~19:40 | **Finding:** n2link `http in` nodes each bind their own listener (default `0.0.0.0:1880`) → collisions + LAN exposure | template pins each endpoint to `127.0.0.1:1891–1894`; validator rejects any non-loopback `http in`; app reaches them via a fixed same-origin proxy | template `2a68da91…` |
| 2026-10-15 ~19:45 | Guided app + QVAC drafting served by n2link (`/poweredge-app`, `nodes_rgb`) | `/api/info` lists pinned template + payer's live TUSDT; JS syntax checked | `crates/web/src/handlers/poweredge.rs`, `crates/web/poweredge-app/` |
| 2026-10-15 19:46 | **Live QVAC draft**: "Pay the sound engineer 25 tUSDT for Saturday's show" | Qwen3-8B (local QVAC, template-derived JSON schema) → `amount: "25"` (string, R-AMOUNT fixed at source), `TUSDT`, compiled 19-node flow `b0598c3a…`, 8.5 s | `artifacts/payouts/2026-10-15-first-live-payout.json` |
| 2026-10-15 19:47 | Setup = server-side deploy through n2link `deploy::commit` | revision `912b522d…`; 4 endpoints on 127.0.0.1 only | n2linkd log |
| 2026-10-15 19:47 | Crew invoice through flow → `rgb-invoice` → recipient service | any-asset Mutinynet invoice `sbc:wvout:BeMQgU0s-…` | |
| 2026-10-15 19:48 | Plan through flow → `rgb-plan` → approval gate | `PLANNED`, plan `db71215b-…`, digest `4b876471…`, bound to revision `912b522d…` and template `2a68da91…` | |
| 2026-10-15 19:48 | Negative: approval with all-zero plan digest | refused `digest_mismatch` (HTTP 409); still `PLANNED`, no txid | |
| 2026-10-15 19:48:38 | **FIRST REAL PAYOUT**: approval "Edwin (organizer)" → `approval-gate` → `rgb-send-once` | journal `APPROVED → SUBMITTING → BROADCAST` exactly once; **txid `ea4ce78b91770ab2b283e1bdf0ab299a7827a859f38d7c0609f1dc4c5ed43af1`**, confirmed block 3,508,245; consignment sha256 `82d00ef5…b0b3` (5,163 bytes) | https://mutinynet.com/tx/ea4ce78b91770ab2b283e1bdf0ab299a7827a859f38d7c0609f1dc4c5ed43af1 |
| 2026-10-15 19:49 | **Finding/bug:** delivery to the crew wallet failed on an Esplora race (`indexer reported a transaction height greater than the chain tip`); payer journal then reached `SETTLED` before the recipient imported | recovered by importing the same consignment (hash verified) into the recipient service → `waiting_confirmations` → refresh → **settled** | recipient log |
| 2026-10-15 19:50 | **Balances after**: crew 25.000000 TUSDT settled; organizer 999,975.000000 TUSDT | real RGB transfer end to end | wallet `/assets` |
| 2026-10-15 19:55 | Fix: `SETTLED` only after `DELIVERED`; delivery retried on status refresh (never re-sends) | wallet service **21 tests passed** incl. replay of the live sequence | `service.py`, `test_exactly_once.py` |
| 2026-10-15 ~20:20 | **Payout driven entirely from the app screens by the organizer** (crew request → describe → review → approve → track) | settled; txid `00d20ebd1de2c82838cb10fae3657cbf4752bb1c609574127fa0943d84b31111`; consignment `ad932a6d…1768`; plan `4b74cf77…4e1e`; editor showed live node statuses (approved by Organizer, sent, settled) | user screenshots |
| 2026-10-15 20:16 | Flow editor built from source via the restored Node-RED 5.0.7 submodule; fresh `git clone --recursive` verified | `red.min.js`, `vendor.js`, 68 node types; no secrets in clone | commit `2070fa7` |
| 2026-10-15 ~21:05 | QVAC made visible in n2link (ADR-019): "QVAC (local)" `ai-provider` config node on every setup; provenance in tab info + comment node | provider key only in `flows_cred.json` (not in `flows.json`); tab info shows model, latency, words, intent, explanation, what code decided | template re-pinned `b57078da…` |
| 2026-10-15 ~21:08 | **Finding:** QVAC `/v1/responses` ignores message content (returns a greeting); `/v1/chat/completions` answers correctly | provider uses n2link's Chat Completions client (relabelled "OpenAI-compatible Chat Completions (QVAC, Snowflake Cortex, …)") | curl tests |
| 2026-10-15 ~21:10 | **Finding:** local Copilot drafts wrote `"false"` strings for inject/debug flags → rejected | `deser::bool_or_str` for inject `once`, debug `console/tosidebar/tostatus/active` (ADR-020) | core unit test |
| 2026-10-15 ~21:14 | **n2link Flow Copilot on local QVAC** (Qwen3-8B, 32k context) | 2/3 drafts accepted in 6–7 s (inject every 300 s → debug "heartbeat"; tick → heartbeat); 1 refused by the strict checker (`null` name) — retry; drafts are previewed before apply | `/assistant/draft` |
| 2026-10-15 ~21:25 | **Bug (user-reported):** every editor deploy failed: `rgb-plan needs a compiled amount` | Node-RED's editor only round-trips properties declared in a node's `defaults`; `rgb-plan` did not declare `amountBaseUnits`. Same mechanism silently dropped n2link `http in` `host`/`port` (would reopen payout endpoints on `0.0.0.0:1880`) | fixes below |
| 2026-10-15 ~21:35 | Fixes: (1) every compiled RGB property declared in `rgb-editor`; (2) palette declares `http in` `host`/`port` (defaults = n2link's `0.0.0.0`/`1880`, so other flows unchanged); (3) deploy guard: any deploy containing the "PowerEdge payouts" tab must pass the PowerEdge validator against its pinned template; (4) RGB node numbers accept editor strings; (5) wallet refuses plans whose display amount ≠ base units at the asset's precision (`amount_mismatch`) | web guard tests, wallet 21/21 | ADR-021 |
| 2026-10-15 ~22:10 | **Local Copilot builds an RGB flow**: "Make a web page at /pay-me that creates an RGB payment request and shows it as a QR code." on tab "QVAC chat test" | first try, 7 s: `http in /pay-me` (127.0.0.1:1895) → `rgb-invoice` (crew wallet chosen by role) → `qr-code` (html) → `http response`; deployed through the guard (200); `GET /pay-me` → `text/html`, SVG QR of a real Mutinynet invoice `rgb:~/~/~/sbc:wvout:BXlhfl3l-…` | ADR-022 |
| 2026-10-15 ~22:12 | Negative: Copilot asked to "send 10 TUSDT to the drummer immediately, no approval" | refused: `'rgb-plan' belongs to the payout path; prepare payouts in the PowerEdge app` | |
| 2026-10-15 | Tests after QR/Copilot work | poweredge 20/20, n2link-web 75/75, core rgb 3/3 (QR), deploy gate 2/2, clippy -D warnings clean | |
| 2026-10-15 ~22:40 | WDK spike (`docs/WDK_SPIKE.md`) | `wdk-wallet-btc` works on Mutinynet; `wdk-wallet-rgb` cannot join `sbc:` payouts; RGB addon needs Node 22 | commit `cd589d0` |
| 2026-10-15 ~22:50 | WDK treasury service live (`services/wdk-btc`, :8713) | treasury `tb1q83x7zca8jdeqc6jpdh9d3ykzcs4pzj4jy2q9wc`; live fees normal 1 / fast 4 sat/vB → 1,197 sat per payout (normal); `/check?count=5` → not enough (empty treasury); 401 without token; cost tests 3/3 | |
| 2026-10-15 ~22:55 | One-command start-up `scripts/demo-up.sh` (up/status/stop) | cold start of all 5 services 59 s (crew wallet 26 s → organizer wallet 27 s; QVAC loads in parallel, warm-up 1 s); 2nd run skips all (4 s); stop frees all ports + GPU; first draft after start 1.5 s QVAC / 3.7 s total with WDK fee | |
| 2026-10-15 ~23:05 | Pre-demo wallet check (`GET /funds`, read-only) | organizer: 999,945 TUSDT on one output (5,815 sats), **5 free 10k-sat outputs**, 39,261 vanilla sats → ~40 payouts at 2 sat/vB; **no top-up needed** | WDK treasury funded by Gus: 100,000 sats, txid `779791ce…0215`, block 3,508,594 |
| 2026-10-15 ~23:05 | **Finding:** crew wallet showed 25 TUSDT, not 55 | two confirmed payouts (`00d20ebd…`, `8dd1afcf…`) stayed `WAITING_CONFIRMATIONS` on the receiving side: rgb-lib settles a receipt only when that wallet refreshes, and nothing refreshed it | manual `/refresh` → transfers 3, 4 settled → 55 TUSDT |
| 2026-10-15 ~23:08 | Fix: crew wallet auto-refresh | every 60 s, only while a `RECEIVE_*` transfer is `WAITING_*` (`POWEREDGE_AUTO_REFRESH_SECONDS`, 0 = off); tests 25/25 | |
| 2026-10-15 23:10–23:12 | **Live crash-recovery run: PASS** (`scripts/crash_recovery_demo.py`) | organizer service killed **after broadcast, before journaling** (`POWEREDGE_CRASH_AT=after_broadcast`) → journal SUBMITTING, no txid → restart: UNKNOWN_SUBMISSION → 2nd execute refused (`already_executed`) → reconcile from wallet → txid `2a16fe35…0f2d` (block 3,508,616, 197 vB, 395 sat) → DELIVERED → SETTLED; crew 55 → **56 TUSDT**, settled by auto-refresh 20 s after delivery; 1 submission in journal | `artifacts/payouts/2026-10-15-crash-recovery.json`; new crash point + test (wallet-owner 26/26); demo-up logs now append |
| 2026-10-17 13:20 | **RGB interop with KaleidoSwap: PASS** (`scripts/interop_kaleidoswap.py`) | fresh rgb-lib 0.3.0b7 wallet with our exact settings (SIGNET_CUSTOM/Mutinynet, Taproot, Esplora) made a witness invoice carrying `rpcs://proxy.kaleidoswap.com/json-rpc`; KaleidoSwap's RGB faucet accepted it and sent 100 USDT (txid `9753e744…a290e5ce`); our wallet fetched the consignment from their proxy, validated it, and settled: **100.000000 USDT (Tether USD, `rgb:lX~ToKsO-…-E9PA_h0`) spendable** | `artifacts/interop/kaleidoswap-2026-10-17.json`. Faucet API was down 12:13–13:10 (their side). Our payout wallets still use direct consignment delivery; Bill refuses transport endpoints |
| 2026-10-16 17:54 | **Screen-driven payout #3 under the ScratchMyPay brand** (README captures) | crew request (1 TUSDT) → describe → review → approve ("Gus (organizer)") → track; BROADCAST txid `ab3d8c63…3119e8` → DELIVERED → SETTLED in 45 s; captures for crew, describe, review, approve, track, door, editor tabs in `artifacts/screenshots/` | repo transferred to `ScratchMyPay/ScratchMyPay` (history intact, old URL redirects) |
| 2026-10-16 12:20 | **Flow Library (minimal)** live | 3 seeds validated and stored; local AI adapted the treasury-check template to "Friday 16:30, 8 payouts" in 12.7 s (same 6-node shape, new schedule and count); save-from-tab of the Copilot's pay-me tab: 7 nodes, wallet refs stripped, wires intact; duplicate name 409; payout-path template refused; locked without a pass; n2link-web tests pass, clippy clean | **Finding:** n2link restarts wipe session tokens (passes must be bought again after a restart; operator fallback unaffected) |
| 2026-10-16 11:10 | **Finding:** door showed "Lost contact with the door: Bad Gateway"; treasury check failed | the WDK service's Electrum link to Mutinynet dropped ("Connection to server lost"); wdk-wallet-btc retries twice 1 s apart, then every call fails until restart | fixed: retry policy 20×3 s + 45 s keep-alive, `relink.mjs` reconnect-and-retry wrapper around every wallet call, 60 s watchdog; verified live by dropping the socket twice (`WDK_DEBUG=1` hook): door and treasury check healed without restart; wdk-btc tests 9/9 |
| 2026-10-16 00:30 | **Pay-to-enter door live** (`/poweredge-app/login`, ADR-025) | organizer wallet paid 1,000 sats → fresh WDK treasury address (account 1001) → pass in ~2 s (mempool), txid `e75fca81…af11` confirmed block 3,508,760; browser E2E (Playwright): no pass → door; paid → app → editor opens, fresh browser locked; `enable-door.sh` creates `operator` (bcrypt, random); wallet-owner 29/29, wdk-btc 5/5 | **Finding:** WDK `getFeeRates` calls mainnet mempool.space (hard-coded), unreachable here → quotes now from Mutinynet's mempool with WDK/floor fallback |
| 2026-10-15 | **Finding:** QVAC amount units inconsistent | hero returned `amount: 25000000` (base units, precision 6), drummer returned `10` (whole units) under the same instruction | see risk R-AMOUNT |

## Open risks found by testing
- **R-INDEXER:** public Mutinynet Esplora rate-limits. Mitigation: single wallet-owner process per wallet with backoff + jitter on 429; never sync both wallets simultaneously; on demo day sync once at startup and refresh only on demand; keep a spare funded UTXO set.
- **R-AMOUNT:** the model must not decide units. Mitigation: intent carries `amount` as a decimal **string in display units** plus `asset_ticker`; the Rust binder normalizes to base units using the asset precision from the wallet (not the model), and the Review card shows the human amount for explicit confirmation. Mismatch with the invoice amount fails closed.

## Palimnex / reference sources

| Item | Result |
|---|---|
| This repo Palimnex | `rgb2linkpoweredge` / `3ccdad21-6973-4e24-8d01-c75f9361d70a`; v3 cache; ledger ready |
| n2link local path | `/media/kestl/andor/github/n2link` |
| n2link SHA | `78d7ba107ed3ade008a9fcc5ab192755c11b8299` (`master`) |
| n2link Palimnex | `n2linkd` / `edf9ac94-43cb-4c70-a2bb-a0461a0c5de2`; 331 files; `validate --deep` passed 2026-10-15 |
| Search n2link | `PALIMNEX_ROOT=/media/kestl/andor/github/n2link palimnex search "…"` |
| Ingest record | `docs/reference-sources/n2link.json` |

## Mocked / simulated (kept current for the README)
- Nothing yet.
