# Architecture decisions and open questions

| ID | Status | Decision |
|---|---|---|
| ADR-001 | Accepted design | Extend n2link; no duplicated new flow engine. |
| ADR-002 | Accepted design | QVAC selects parameterized, pinned templates; never arbitrary privileged flow generation. |
| ADR-003 | Accepted design | AI/planner/runtime separated from financial signer and journal. |
| ADR-004 | Accepted design | Native process and OCI capsules, not bitcoind in Wasmi; Firecracker deferred. |
| ADR-005 | Accepted design | rgb-lib-python from Bill is first real wallet backend; WDK optional independently owned wallet. |
| ADR-006 | Accepted design | Visual RGB dApps, not unrestricted on-chain smart contract compilation. |
| ADR-007 | **Accepted 2026-10-15** | Hybrid. RGB nodes live **inside `n2link-core` behind `nodes_rgb`** (like `nodes_ai`), not a plugin crate: `BaseGlobalNodeState` fields are crate-private, so a config node (`rgb-wallet-ref`) cannot be built outside core. The nodes are unprivileged HTTP clients; all keys and spending live in the Python wallet-owner service on loopback (one process per wallet datadir). |
| ADR-008 | **Accepted 2026-10-15** | QVAC CLI 0.15.0 + SDK 0.21.0 installed together in `services/qvac`, `qvac serve --openai` on 127.0.0.1:11435 with bearer key, Qwen3-8B-Q4_K_M, `json_schema` response format. |
| ADR-009 | **Accepted 2026-10-15** | Mutinynet (custom signet) via Esplora `https://mutinynet.com/api`. Electrum is rejected by rgb-lib 0.3.0b7 (`InvalidIndexer`). Regtest is post-event. |
| ADR-010 | Open | Accepted escrow script policy and test fixture design for MergePay (later). |
| ADR-011 | Accepted | Pure `poweredge` crate (intent, registry, compiler, validator). Node IDs = first 8 bytes of sha256(template_digest ‖ intent_digest ‖ workspace ‖ ref). Placeholders must be whole strings. |
| ADR-012 | Accepted | Payout approval is recorded by the wallet-owner journal, bound to `{plan_digest, flow_revision, actor}`; `rgb-approve` records it, `approval-gate` only waits for it, `rgb-send-once` re-checks the flow revision. A redeploy voids pending approvals. |
| ADR-013 | Accepted | The guided app talks only to n2link flow endpoints (`/poweredge/*`), never to the wallet service. |
| ADR-014 | Accepted | One public repo; n2link imported as a squashed subtree under `n2link/` at `78d7ba1` (declared prior work). |
| ADR-015 | Accepted 2026-10-15 | Amounts: the model supplies a display-unit decimal string; the compiler converts with the precision the wallet reports. Tickers are normalized to uppercase (rgb-lib rule) before an exact match. |
| ADR-016 | Accepted 2026-10-15 | First-time recipients issue **any-asset** invoices (rgb-lib refuses to name an asset the wallet never held). The payer still pins its own contract and refuses invoices that name a different asset. |
| ADR-017 | Accepted 2026-10-15 | Durable journal before every irreversible step: `SUBMITTING` is committed (SQLite WAL, `synchronous=FULL`) before the send; restart turns it into `UNKNOWN_SUBMISSION`, reconciled from wallet transfers, never resent. Plans approved before a service restart cannot execute (Bill's adapter keeps confirmations in memory) and must be re-planned: fail closed. |
| ADR-019 | Accepted 2026-10-15 | QVAC stays **outside** the money path (it drafts before the flow exists) but is **visible in n2link**: each setup adds an `ai-provider` config node "QVAC (local)" (key in the encrypted credential sidecar) used by n2link's Flow Copilot, and stamps provenance (model, latency, the organizer's words, the structured intent, the AI explanation, what code decided) into the tab info and the template's comment node via `{{ctx:provenance}}`. Model text is defanged so it can never form a placeholder. Provider kind is n2link's generic OpenAI **Chat Completions** client (`cortex` value, relabelled in the editor): QVAC's `/v1/responses` ignores message content (tested). |
| ADR-020 | Accepted 2026-10-15 | n2link core: inject `once` and debug `console`/`tosidebar`/`tostatus`/`active` accept `"true"`/`"false"` strings as well as booleans (`deser::bool_or_str`), as Node-RED flows in the wild do. Found because local QVAC Copilot drafts wrote `"false"`; every other Copilot check stays strict. |
| ADR-021 | Accepted 2026-10-15 | The "PowerEdge payouts" tab is **managed**: n2link's deploy path (`deploy::commit`, used by the editor, the API and the app) re-validates it against the pinned template it was compiled from (found by `rgb-plan.templateDigest`). Editor deploys of other tabs are unaffected; edits that skip approval, add nodes or reopen endpoints on the network are refused with a plain message. The wallet independently refuses a plan whose displayed amount differs from the base units it would send. |
| ADR-018 | Accepted 2026-10-15 | Each RGB node is bound to a wallet role: `rgb-invoice` only accepts the recipient wallet; plan/approve/gate/send/refresh/status only the payer wallet. |

Record implementation findings as new ADRs rather than silently overriding these choices. All stated components are proposed until implemented and tested.
