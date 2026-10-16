# Licenses

This repository is **source-available**. Original ScratchMyPay components are under the
**PolyForm Noncommercial License 1.0.0** (`LICENSE`). Third-party and upstream components keep
their own licenses, listed below. Attribution requirements are in `NOTICE`.

## Original ScratchMyPay components — PolyForm Noncommercial 1.0.0

| Component | Paths |
|---|---|
| PowerEdge request format, template registry, compiler, validator, deploy gate | `n2link/crates/poweredge/`, `templates/`, `n2link/crates/web/src/handlers/poweredge.rs`, `n2link/crates/web/src/handlers/flow_library.rs` |
| RGB workflow nodes and WDK nodes for n2link, editor palette, Flow Copilot patterns for them | `n2link/crates/core/src/runtime/nodes/rgb_nodes/`, `n2link/crates/web/rgb-editor/`, `assistant-skills/` additions for RGB/WDK |
| Guided app, pay-to-enter door, editor theme and brand assets | `n2link/crates/web/poweredge-app/`, `n2link/assets/theme/` (except the fonts), `n2link/assets/brand/` |
| Wallet-owner service and durable journal | `services/wallet-owner/` |
| WDK treasury service and door | `services/wdk-btc/` |
| QVAC service wrapper, provisioning, demo and evidence scripts, docs, seeds | `services/qvac/`, `scripts/`, `docs/`, `library/`, `examples/`, `artifacts/` |
| MVM orchestration (planned, `docs/SATURDAY_PLAN.md`) | `scripts/vm/` when present |

Permitted: noncommercial use, including research, education, experimentation, modification and
redistribution under the license terms. **Commercial use** (production deployments, paid hosting,
resale, commercial integrations outside the PolyForm Noncommercial permissions) requires separate
written authorization from the copyright holders. Contact the project maintainers.

## Upstream and third-party components — their own licenses

| Component | Where | License |
|---|---|---|
| n2link runtime and editor (fork of oldrev/edgelinkd), prior work at `78d7ba1` | `n2link/` (everything not listed above) | Apache License 2.0 — `n2link/LICENSE`, `n2link/NOTICE` |
| Node-RED editor client (built from source, submodule) | `n2link/3rd-party/node-red/` | Apache License 2.0 |
| RGB Bill wallet adapter (prior work, separate repository, pinned `be3d237`) | cloned next to this repo | as set in that repository |
| rgb-lib / rgb-lib-python 0.3.0b7 (RGB-Tools) | Python dependency | MIT |
| RGB protocol libraries (rgb-protocol, via rgb-lib) | transitive | Apache License 2.0 / MIT, per crate |
| Tether WDK `@tetherto/wdk-wallet-btc` 1.0.0-beta.17 | `services/wdk-btc/node_modules` (not committed) | Apache License 2.0 |
| QVAC `@qvac/sdk` 0.21.0 and `@qvac/cli` 0.15.0 | `services/qvac/node_modules` (not committed) | Apache License 2.0 |
| Qwen3-8B model weights (downloaded by QVAC, not committed) | `.qvac/` | model's own license (Apache License 2.0 as published by its authors) |
| Inter, Space Mono, Anton fonts (via Fontsource) | `n2link/assets/theme/fonts/` | SIL Open Font License 1.1 — `LICENSE-*.txt` there |
| Rust and Node dependencies | `Cargo.lock`, `package-lock.json` | each crate/package's own license (MIT / Apache-2.0 / BSD families) |
| Firecracker (planned for the micro-VM work; downloaded at setup, not committed) | `scripts/vm/` | Apache License 2.0 |

Where an original component is a modification of an Apache-2.0 file inside `n2link/`, the
upstream Apache-2.0 notices stay in place and apply to the upstream portions; the PolyForm terms
apply to the ScratchMyPay additions. This split is documented here rather than per file; a per-file
header pass is on the post-event list.
