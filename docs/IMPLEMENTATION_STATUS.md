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
| Bill (dependency) | `EdwinKestler/bill@be3d237` |

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
