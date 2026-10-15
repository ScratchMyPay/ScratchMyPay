# ROADMAP — execution for local Codex

## M0 — inspect and baseline (no functional change)
- Checkout pinned n2link revision (`master` is not a substitute for a SHA). Read root AGENTS, runtime plugin/node contracts, tests, Wasmi ADR, security docs.
- Inspect RGB Bill's wallet flow and create a reuse/dependency map; no secret/file copying.
- Record Rust version, features, tests, hardware and QVAC available version in `docs/IMPLEMENTATION_STATUS.md`.
- Establish red/green test harness for template parsing/validation using fixtures.
**Exit:** upstream tests preserved; detailed capability report, reproducible commands and a small ADR for extension strategy.

## M1 — AI-assisted validated flow draft (highest priority)
- Implement strict `WorkflowIntent` schema and untrusted QVAC adapter with real local smoke proof.
- Implement **one** pinned `rgb-payment-v1` template, deterministic IR/compiler, validator, editor preview/diff; no funds movement.
- Reject arbitrary node types, unsafe edges, missing approval gate, unknown network, unknown capability, unsupported asset, interpolated secrets.
- Require explicit deployment approval; no automatic deployment after AI output.
**Exit:** same intent + template produces byte-identical canonical graph; independent invalid-input tests; n2link loads graph without breaking compatibility; demo starts with prompt.

## M2 — real RGB backend (choose one test environment)
- Implement isolated wallet-owner service (reuse tested Bill rgb-lib path through narrow typed protocol) and Rust node/gateway client.
- Confirm real Mutinynet **or** regtest RGB asset issuance/transfer using independently isolated datadirs. Do not substitute plain Bitcoin tx for RGB proof.
- Implement read/prepare/approve/submit/reconcile; persist idempotency journal before external submission; define unknown-submission recovery.
- Verify actual invoice, recipient consignment import, wallet settlement and txid.
**Exit:** real transaction and hashes/receipts captured; crash tests prove no blind duplicate send.

## M3 — approved service capsule
- Native process supervisor with exact binary allowlist and private endpoints for Bitcoin regtest + minimal readiness; optionally electrs. No arbitrary command executor node.
- Add OCI only after native supervisor security tests. Firecracker remains post-event.
**Exit:** isolate, create, start, observe, stop, remove, recovery, safe cleanup; no port collisions.

## M4 — reusable applications and WDK
- `rgb-proofpay-v1`, `rgb-splitpay-v1`, later MergePay.
- Separate WDK adapter with network/schema/OOB compatibility tests; no shared wallet datadir.
- Lightning and RGB contract compiler remain independent research tracks.

## Team parallelization (4 contributors)
A: n2link native node/editor integration; B: RGB owner/gateway + wallet integration; C: QVAC adapter/template compiler; D: verification/security/capsules. Maintain one named integration owner. Cross-cutting tests before a showcase.

## Scope discipline
M0, M1, M2 and the guided UX are all Sunday deliverables and run as parallel tracks (see `docs/IMPLEMENTATION_PLAN.md`). M3/M4, Lightning, an RGB contract compiler and Firecracker are post-event; WDK is a time-boxed bonus spike only. Spend time on whatever most improves the live Sunday demo.
