# RGB2link PowerEdge — Code Reuse Map

Status: implementation guidance, 2026-10-13. Local n2link pin 2026-10-15: `/media/kestl/andor/github/n2link` at `78d7ba107ed3ade008a9fcc5ab192755c11b8299`. No code has been copied or integrated by this document. Validate every interface against that pinned commit before modifying n2link. See `referencerepos.md` and `docs/reference-sources/n2link.json` for sources and Palimnex search.

## Strategy and source of truth

**Extend n2link rather than fork or rewrite its engine.** Reuse n2link's Node-RED editor/runtime, deployment controls, policy protections, and optional WASM sandbox; add a narrowly scoped Rust RGB node pack and an isolated financial execution service. Reuse RGB Bill's demonstrated wallet and BLE patterns without importing its wallet data or keys. QVAC generates *template IDs + typed parameters*, not privileged code or arbitrary flow JSON. The compiler validates and expands approved templates to n2link flows, previews them, and requires authorization before deployment.

There are four trust domains: (1) AI and ordinary flow nodes, (2) approved template compiler/policy, (3) privileged wallet + signing + chain services, (4) external GitHub/BLE/Bitcoin connections. Nothing in (1) may bypass (2) to obtain signing rights in (3).

## Reuse decisions

| Subsystem | Provenance / starting location | Reuse | Changes necessary | Do **not** do | Acceptance evidence |
|---|---|---|---|---|---|
| Runtime/event graph | Local n2link `78d7ba1`: `crates/core/src/runtime/{engine.rs,flow.rs,nodes/}`, `crates/macro/src/lib.rs` (`#[flow_node]`) | Extend existing Rust node API; keep native message and lifecycle conventions | Typed node interfaces, error/status ports, capability checks | Build a competing graph runner | Import, execute, restart a flow |
| Editor + deployment | Local n2link `78d7ba1`: `crates/web/src/handlers/flows.rs`, browser editor, `flows.json` | Existing editor and deploy endpoints | RGB node palette/parameters, template preview, approval UX | Expose raw signer methods to browser or assume deploy equals spending consent | Deploy a non-spending example |
| Template generation | `n2link` flow JSON + starter `schemas/`, `templates/` | Template compiler and registry; exact pinned versions | Canonical typed intermediate representation, graph validation, signed template manifest | Accept AI-authored arbitrary node JS / credentials / shell commands | Deterministic generation given same template and inputs |
| Local AI | n2link AI nodes + QVAC SDK installed separately | Existing AI orchestration; QVAC local worker as provider | Strict schema output and evidence/model-version metadata | Use cloud inference while claiming local QVAC; let model authorize payments | Network-disabled inference smoke test |
| Persistence/recovery | `n2link`: optional SQLite history and context; `bill`: retained artifacts/recovery patterns | Reuse storage patterns, build a **separate durable financial journal** | Transaction states, idempotency key, restart reconciliation | Treat generic Node-RED context as sufficient exactly-once financial state | Inject crash after submit, no unintended resend |
| Credentials/security | `n2link`: encrypted sidecars, role-based admin, ingress/egress policy | Apply existing credential storage and network controls | Capability-scoped wallet service, mainnet deny-by-default, approval bound to plan hash + flow revision | Store seeds in `flows.json`, logs, prompts or WASM guest | Secret leakage and unauthorized flow tests |
| WASM nodes | Local n2link `78d7ba1`: `adoption/phase7/ADR-0002-wasm-node-sdk.md`, optional `nodes_wasm` | Pure computation, policy calculations and validation helpers | Manifest, pure deterministic ABI | Sign, access network, files, wallet credentials from WASM guest | Reject forbidden host imports |
| RGB wallet flows | `EdwinKestler/bill`: `web/rgb/`, `scripts/rgb_tap.py`, `scripts/rgb_wallet.py`, `docs/RGB-TAP-LIVE.md` | **Extract patterns and tested operations**, keep existing Python adapter as bridge at first | Narrow IPC/API boundary and wallet ownership locks; separate role datadirs | Copy `.env` files, mnemonic, keys or wallet database; open same wallet in two processes | Reproduce known Mutinynet transfer on isolated test wallets |
| BLE/OOB transport | `bill`: `dock/`, `lib/DockGatt`, `lib/TapGatt`, `docs/PROTOCOL.md` | Reuse framing, chunk ACK and artifact format for optional RGB Bill integration | Persistent metadata restoration, authenticated transport, digest/receipt checks | Treat CRC32 as authentication or BLE delivery as final settlement | Byte-for-byte consignment roundtrip |
| RGB native library | `RGB-Tools/rgb-lib`, matching bindings | Call established issuance, invoices, sends, OOB consignment import, refresh methods | Version-pinned wallet service adapter | Invent a new RGB wallet or combine incompatible `rgb-lib` versions | Real on-chain send/receive/refresh test |
| WDK adapter | `UTEXO-Protocol/wdk-wallet-rgb` | Separate experimental Node.js service | Feature probing (network, schema, raw OOB bytes), per-network isolated data | Assume WDK Signet and SignetCustom interoperability or share wallet dirs | Capability/compatibility report; no claim until actual transfer |
| Lightning / HODL | `RGB-Tools/rgb-lightning-node`, UTEXO fork | Research-only optional service capsule | Regtest isolated integration, HTLC persistence and claim/cancel test | Make Lightning a blocker for M1 or use HODL as indefinite escrow | Independent regtest integration suite |
| Bitcoin lab | `bitcoin/bitcoin`, `romanz/electrs`, `RGB-Tools/rgb-proxy-server` | Reference configs/tests and provision **installed/pinned binaries or containers** | Capsule supervisor, constrained endpoints, readiness + snapshots | Vendor Bitcoin Core or pretend Wasmi runs native bitcoind | Launch, mine, index, stop, restart in regtest |
| Service Capsules | `n2link`: process policies and runtime primitives; Bitcoin sample configs | New approved-catalog Rust supervisor using native/OCI backends | Sandboxed launch, ports, data dir isolation, quotas, lifecycle/health | Arbitrary shell/exec from untrusted flows; call subprocess a hardware VM | Regtest health endpoint and clean teardown |
| Evidence/attestation | `bill`: consignment SHA-256 and receipt evidence; n2link history | Content-addressed evidence and signed receipts | Define claims and distinguish transport, RGB valid, Bitcoin settled | Claim a hash proves real-world truth | Tampering invalidates receipt |
| GitHub MergePay demo | GitHub REST API + future adapter | Read-only PR/merge/CI ingestion | Scoped GitHub token, attributable snapshots, ranking manifest | Trust line count or AI score as authorization | Deterministic fixture test |
| Palimnex indexing | Installed `palimnex` 2.9.0; n2link corpus at `/media/kestl/andor/github/n2link` | External project knowledge index | Source commit inventory and retrieval metadata | Ingest private secrets, wallet state, `.env`, model weights, chain databases; merge another repo's Redis into this project | Queries retrieve canonical implementation locations |

## Integration order (dependency order; M1, M2 and UX are built in parallel — the one hard rule is that a real send is never enabled before the approval gate and journal pass tests)

1. **M0 — Inventory**: pin the n2link SHA, verify its node-pack extension API, verify bill commit and wallet bindings; generate a capability matrix. Preserve all existing n2link tests.
2. **M1 — Flow authoring**: QVAC offline structured intent -> allowlisted template -> typed IR -> deterministic `flows.json` -> validate -> preview in editor. No signing, mainnet, or wallet writes.
3. **M2 — Wallet bridge**: create a separate service process reusing bill's known-good `rgb-lib-python` method. Implement query, invoice, immutable plan, approve, submit-once, reconcile. Keep explicit env/role isolation.
4. **M3 — Regtest Capsule**: supervisor launches pinned bitcoind + electrs (where necessary) with isolated folders/ports and readiness. Add deterministic fixtures, mine blocks, stop/cleanly reset.
5. **M4 — Optional capabilities**: WDK feature probe, RGB proxy/Lightning, BLE relay, PR-analysis template; do not blend these into the critical-path transaction.

## Financial invariants (required for any reuse)

- No private keys in flow JSON, QVAC prompts, observability logs, Git history or Palimnex index.
- A *flow deployment* is not a *payment authorization*. Approval binds an immutable canonical payment plan, wallet ID, network, amount, recipient, signer scope, expiry, template version and deployed flow revision.
- An uncertain send outcome must be reconciled, never automatically repeated. Persist intent and result before acknowledging outward API requests where applicable.
- “Artifact delivered”, “RGB consignment validated”, “Bitcoin confirmed”, and “settled” are distinct states.
- Never share RGB wallet database ownership across processes, backends, or restored snapshots.
- Regtest and Mutinynet configurations must not silently coerce one another (especially `signet` vs `signetcustom`).
- WDK and Lightning are optional adapters until proven compatible on the selected environment.
- Any third-party reuse requires original license and NOTICE attribution; inspect exact versions and submodules before copying code.

## Coding-agent execution rule

For any new capability: **inspect -> cite exact file and commit -> state reuse/extension decision -> implement minimal adapter -> unit test -> integration test -> update `docs/DECISIONS.md` and `docs/ACCEPTANCE.md`.** Never claim a feature is working based solely on documentation or mocked tests.
