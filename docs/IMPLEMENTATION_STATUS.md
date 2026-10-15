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
| 2026-10-15 | Payer funding address | `tb1pncw03gkwd65gydgq2pq499xq09nsdymhp6eara5cq9de8m9mjdzs3dd26s` | awaiting faucet funds |
| 2026-10-15 | Recipient funding address | `tb1pglk2u0d64upreatyl6sz3sy7yny583l4qu40rh092gg0jhdkfngqyt6t4e` | awaiting faucet funds |

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
