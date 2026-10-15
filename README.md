# RGB2link PowerEdge by n2link

**Describe. Validate. Deploy.** A proposed AI-assisted, visual development environment for Bitcoin/RGB applications using the existing [n2link](https://github.com/n2link/n2link) Rust/Node-RED-compatible runtime, locally running QVAC, and verified workflow templates.

> **Status: design and implementation starter, not a deployed or tested product.** This package describes target interfaces and acceptance gates. It does not claim that RGB nodes, QVAC composition, or Capsule services are already implemented.

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

## Initial milestone: M0/M1
**M0**: reproducible development environment and design-contract checks; report exact n2link revision; no fund movements. **M1**: a local QVAC inference selects an allowlisted template, emits typed parameters; a deterministic compiler and validator generate a valid n2link `flows.json` draft, which appears in the editor; dry-run only. **M2**: integrate a real isolated RGB wallet to create an invoice and execute one separately approved Mutinynet/regtest transaction, inspect settlement. **M3**: optional controlled regtest capsule supervisor with explicit operator authorization. Do not attempt M2/M3 before M1 safety gates pass.

## Quick start for local Codex
1. Create a working feature branch of n2link; capture base SHA, compiler and platform. Keep existing runtime behavior and tests intact.
2. Copy or mount this specification into your checkout, **without overriding existing `AGENTS.md`**. Read `AGENTS.md`, `docs/DESIGN.md`, `docs/ROADMAP.md` and `prompt.md`.
3. Begin with `prompt.md`; inspect real extension points before editing. First response from Codex must document findings, a minimal plan, risks and chosen module boundaries.
4. Build M0/M1 in small commits; run baseline and targeted tests; do not touch real wallets or mainnet by default.
5. See `docs/ACCEPTANCE.md` for exit gates and `docs/SECURITY.md` for fail-closed requirements.

## Suggested demo
Prompt → QVAC structured intent → registry selects `rgb-payment-v1` → parameter binding → validator → editor preview → user-approved deployment → invoice → authorized wallet send → recipient import/refresh → signed receipt. Under the first milestone stop at **editor preview**; demonstrate settlement only after the wallet adapter is proven against an isolated test environment.

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

## Hackathon provenance
Clearly separate pre-existing n2link/Bill functionality from work committed during Oct 17–18, 2026. Do not imply an existing runtime or wallet integration was authored during the event. Keep explicit evidence (commit SHA, tests, txid and local model runtime logs) of new work.

## License
Recommended Apache-2.0 for the new package, subject to checking upstream dependency licenses. Preserve n2link/EdgeLinkd attribution. No repository or code has been changed by this document bundle.


## Source reuse and Palimnex knowledge base

Read [`reusecode.md`](reusecode.md) before editing third-party or existing project code. Read [`referencerepos.md`](referencerepos.md) for verified repository URLs, safe clone commands, pinned checkout metadata, and index exclusions. Both are mandatory references for local Codex.
