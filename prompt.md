# INITIAL CODEX PROMPT — RGB2link PowerEdge

You are the implementation engineer for **RGB2link PowerEdge by n2link**, an AI-assisted, Rust-based visual programming environment for Bitcoin/RGB applications. Work inside my local n2link checkout. The attached starter folder is a **specification**, not pre-existing implementation. Never assert features exist without inspecting code.

## First actions
1. Read the existing n2link `AGENTS.md`, the new starter `AGENTS.md`, `README.md`, `docs/DESIGN.md`, `docs/ROADMAP.md`, `docs/SECURITY.md`, `docs/ACCEPTANCE.md`, `docs/DECISIONS.md`, and relevant `.agents/skills/*/SKILL.md`.
2. Inspect live Git state/branch, `Cargo.toml`, Rust node registration/trait APIs, built-in editor integration, existing AI support, Wasmi plugin boundaries, credential and egress policy, tests, and existing flow JSON fixtures. Do not assume the tree equals any previously reviewed version.
3. Inspect RGB Bill's wallet adapter interfaces locally if available; otherwise report missing access and use a clearly marked stub for **dry-run-only** M1. Never copy keys/datadirs.
4. Produce a concise baseline report and ADR choosing the minimum-change extension pattern before editing implementation code.

## Primary deliverable: the full Sunday product (M0 + M1 + M2 + guided UX), per `docs/IMPLEMENTATION_PLAN.md`
- Define `WorkflowIntent` type and strict JSON validation (use schemas/workflow-intent.schema.json as proposed contract).
- Build template registry with a single pinned `rgb-payment-v1` manifest and verified digest.
- Implement a deterministic parameter binder and compiler into actual supported n2link `flows.json` representation, after inspecting existing parsers. Use type-safe, restricted nodes and an explicit approval gate. No user-supplied unrestricted script or arbitrary service execution.
- Implement validator for allowlisted nodes, graph edges, network tags, wallet refs and spending capabilities; fail closed.
- Connect **real local QVAC** as an untrusted intent interpreter. Verify SDK/runtime availability. If absent, complete deterministic fixture-based compiler and leave the integration blocked with exact missing pieces; do not fake AI output.
- Show the draft in the existing n2link editor and in the guided app; preview and user approval required for deployment. The flow-authoring path itself never moves funds.
- Add unit, property/negative and editor integration tests. Run and report them.

## Wallet track (in parallel with M1)
Implement M2 with an isolated RGB wallet service using the already accepted Bill `rgb-lib-python` workflow, developed alongside M1 against a fake backend first, then live Mutinynet. Require one-use approval, durable idempotency/reconciliation and a genuine recipient import/settlement. The real send path is enabled only once its approval gate and journal tests pass. WDK is a time-boxed bonus spike; Lightning/multi-node regtest/Firecracker are post-event. Financial actions are gated and testnet-only.

## Guided UX track (in parallel)
A non-technical user completes Describe → Review → Approve → Track without the flow editor. Every step shows plain-language status; every error says what happened and what to do next.

## Concrete acceptance evidence
- Model/prompt → valid structured intent → exact registry template/digest → reproducible validated graph → visible preview → explicit approved deploy.
- Negative tests: prompt injection, unknown template, wrong network, missing approval gate, changed graph after approval, unsafe node, wallet secrets and duplicate action.
- If M2 done: real RGB contract ID, invoice, consignment digest, Bitcoin txid, settled recipient balance, no duplicate execution under forced crash (redact secrets).

## Constraints
- Use Rust and established n2link node interfaces where appropriate. Sidecar service acceptable where required for wallet/SDK compatibility.
- Keep pre-existing n2link/EdgeLinkd provenance; avoid broad refactors or license changes.
- Do not rewrite the editor, add generic VM hosting in M1, or promise arbitrary RGB smart contract execution.
- Do not claim `signetcustom` and `signet` are interchangeable; validate capabilities per backend.
- Work incrementally, make narrow commits and create `docs/IMPLEMENTATION_STATUS.md` as a factual ledger.

Now start with baseline inspection, then work the parallel tracks in `docs/IMPLEMENTATION_PLAN.md` toward the Sunday demo. If the repository differs from this design, document the divergence and choose the safest path that preserves the contracts above.
