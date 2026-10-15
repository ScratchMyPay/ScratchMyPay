# RGB2link PowerEdge by n2link

**Describe. Validate. Deploy.** A proposed AI-assisted, visual development environment for Bitcoin/RGB applications using the existing [n2link](https://github.com/n2link/n2link) Rust/Node-RED-compatible runtime, locally running QVAC, and verified workflow templates.

> **Status: hackathon build in progress.** Work started 2026-10-15 with the organizers' approval to begin early on a public repository. Target: a complete, live, end-to-end product on Sunday 2026-10-18 13:00 CEST. Nothing below is claimed as working until `docs/IMPLEMENTATION_STATUS.md` records the test or live run that proves it.

## Hackathon submission — "Agentic Dollars on Bitcoin", Turin, Oct 17–18, 2026

**Project:** RGB2link PowerEdge · **Team:** n2link
**Members:** Edwin Kestler ([@EdwinKestler](https://github.com/EdwinKestler)) · [@cms](https://github.com/cms)

**Idea.** Post-show payouts for live events. After a concert, an organizer has to pay the sound engineer, the stage crew and the artist. With PowerEdge they type it in plain words ("Pay the sound engineer 25 tUSDT for Saturday's show"); a local AI (QVAC) turns that into a validated payment workflow, shows a plain-language plan, and the organizer approves it. The payment settles as an RGB asset transfer on Bitcoin (Mutinynet test network), with a receipt both sides can verify. Payout details never leave the organizer's machine.

**Stack.** Rust: [n2link](https://github.com/n2link/n2link) (Node-RED-compatible runtime, editor, Flow Copilot). Python: `rgb-lib` 0.3.0b7 wallet service (FastAPI). Local AI: QVAC (`qvac serve --openai`, structured JSON output). Network: Mutinynet (`signetcustom`). Guided web app served by n2link. WDK: under evaluation (bonus).

**Installed and working before the event (prior work, declared):**
- n2link runtime, editor, Flow Copilot, credential storage, egress policy — `n2link/` at `78d7ba107ed3ade008a9fcc5ab192755c11b8299` (fork of oldrev/edgelinkd, Apache-2.0), imported as one squashed commit.
- RGB Bill (`EdwinKestler/bill` @ `be3d237`): `rgb-lib-python` wallet adapter with plan → confirm → execute-once, proven on Mutinynet; QVAC SDK smoke test. Used as a pinned dependency, not copied.
- Palimnex 2.9.0 (code search / agent memory tooling).
- This repository's design docs, schemas and plan (`docs/`).

**Built for the hackathon** (organizer-approved early start on 2026-10-15): everything not listed above — tracked commit by commit and summarized in `docs/IMPLEMENTATION_STATUS.md`. Mocked/simulated parts will be listed here before submission.

## Product promise
A developer asks: “Create a testnet RGB payment API with explicit approval, a receipt, and local AI interpretation.” Local QVAC selects a pinned, validated template and fills typed parameters. An independent validator rejects unsupported nodes, unsafe bindings, policy bypasses and unsupported wallet/network combinations. The editor displays the proposed flow. The user approves a versioned deployment. n2link runs the workflow against a privileged, isolated wallet service; immutable audit records and a reconciliation process prove the outcome.

## Non-goals
- Not an Ethereum-style on-chain smart-contract deployment platform: RGB is client-side validated, anchored to Bitcoin single-use seals.
- Not a production self-custody wallet or permissionless signing agent in v0.
- Not Bitcoin Core inside the n2link Wasmi plugin sandbox: `bitcoind`, `electrs`, wallets and QVAC are native processes or containers.
- Not an autonomous authorization system: QVAC cannot sign, transfer funds or deploy arbitrary executables.
- No claims of offline Bitcoin finality.

## Existing source and reuse
- [n2link/n2link](https://github.com/n2link/n2link): Rust runtime, integrated editor, Node-RED-compatible flows, auth, credential storage, egress controls, experimental Wasmi sandbox. It is a fork of oldrev/edgelinkd (Apache-2.0); retain provenance, LICENSE and NOTICE.
- [EdwinKestler/bill](https://github.com/EdwinKestler/bill): proven on-chain RGB `rgb-lib-python` wallet path on Mutinynet and byte-exact BLE artifact transport. Extract adapter patterns; **do not copy wallet state, credentials, seed or device-specific paths**.
- WDK/UTEXO remains optional, separately isolated and behind compatibility tests.

## Sunday deliverable (M0 + M1 + M2, built in parallel)
**M0**: reproducible environment; exact n2link/Bill revisions recorded. **M1**: local QVAC selects an allowlisted template and emits typed parameters; a deterministic compiler and validator generate a valid n2link flow shown in the editor and in the guided app. **M2**: a real isolated RGB wallet pair on Mutinynet: invoice, one human-approved send, recipient import, settlement and receipt. **UX**: a guided app (Describe → Review → Approve → Track) a non-technical user can complete without the flow editor. M1 and M2 are developed in parallel tracks; the only hard rule is that no real send is wired up without the approval gate and journal in place. **M3** (capsules) is post-event.

## Quick start for local Codex
1. Create a working feature branch of n2link; capture base SHA, compiler and platform. Keep existing runtime behavior and tests intact.
2. Copy or mount this specification into your checkout, **without overriding existing `AGENTS.md`**. Read `AGENTS.md`, `docs/DESIGN.md`, `docs/ROADMAP.md` and `prompt.md`.
3. Begin with `prompt.md`; inspect real extension points before editing. First response from Codex must document findings, a minimal plan, risks and chosen module boundaries.
4. Build in small, frequent commits pushed to the public repo; run baseline and targeted tests. Test networks only (Mutinynet/regtest); never mainnet.
5. See `docs/ACCEPTANCE.md` for exit gates and `docs/SECURITY.md` for fail-closed requirements.

## Suggested demo
Prompt → QVAC structured intent → registry selects `rgb-payment-v1` → parameter binding → validator → plain-language review + editor preview → user-approved deployment → invoice → authorized wallet send → recipient import/refresh → receipt. The full chain runs live in the 3-minute judge demo on Mutinynet.

## File map
- `docs/DESIGN.md` — architecture, data/control planes and contracts
- `docs/ROADMAP.md` — dependency-aware milestone plan
- `docs/SECURITY.md` — threats and explicit trust boundaries
- `docs/ACCEPTANCE.md` — automated and manual test gates
- `docs/DECISIONS.md` — architecture decisions / unresolved questions
- `prompt.md` — first Codex execution prompt
- `AGENTS.md` — local agent instructions
- `SKILLS.md` — skill catalog and routing
- `.agents/skills/*/SKILL.md` — modular agent skills
- `schemas/workflow-intent.schema.json` — proposed constrained QVAC output schema
- `templates/rgb-payment-v1.template.json` — **illustrative template manifest, not deployable Node-RED flow**

## Hackathon provenance (declared prior work)
Prior work, not authored for this hackathon: n2link runtime/editor/Flow Copilot at `78d7ba107ed3ade008a9fcc5ab192755c11b8299` (fork of oldrev/edgelinkd, Apache-2.0); RGB Bill wallet adapter at `be3d237`; Palimnex 2.9.0 tooling. Everything else is new work since 2026-10-15 (organizer-approved early start). The Friday 23:00 snapshot and this README list what existed at that point; commits after it are the in-event delta. Keep evidence (commit SHAs, tests, txids, QVAC runtime logs) and list anything mocked, simulated or hard-coded.

## License
Recommended Apache-2.0 for the new package, subject to checking upstream dependency licenses. Preserve n2link/EdgeLinkd attribution.


## Source reuse and Palimnex knowledge base

Read [`reusecode.md`](reusecode.md) before editing third-party or existing project code. Read [`referencerepos.md`](referencerepos.md) for verified repository URLs, safe clone commands, pinned checkout metadata, and index exclusions. Both are mandatory references for local Codex.
