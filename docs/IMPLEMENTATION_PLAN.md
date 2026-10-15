# IMPLEMENTATION PLAN — RGB2link PowerEdge (Sunday delivery)

> **Status 2026-10-16:** tracks A–D and the first live payouts were completed on Oct 15 (see `IMPLEMENTATION_STATUS.md`). Saturday's hackathon-day plan is in **`SATURDAY_PLAN.md`** (wallets in Rust micro-VMs).

Status: active plan, rev 2, 2026-10-15 18:30 CEST · Base: n2link `78d7ba107ed3ade008a9fcc5ab192755c11b8299` (`0.4.0-rc.1`) · Bill `be3d237`.
Change from rev 1: organizers approved starting now on a public repo, so there is no "pre-event, no code" phase and no "stop after M1" gate. M1, M2 and the guided UX are built in parallel and all ship Sunday. Safety invariants (`SECURITY.md`) are unchanged: test networks only, human approval bound to plan + flow revision, journal before send, no secrets in git.

## 1. What we deliver Sunday 13:00 CEST

A **live** product, demoed at the desk in ~3 minutes, that a non-technical user can drive:

1. **Guided app** (Describe → Review → Approve → Track) with plain-language steps and errors. Two views for the live demo: *Payer* and *Recipient*.
2. **Local QVAC** turns the user's words into a pinned template + typed values, and writes a plain-language explanation of the plan. Runs on this machine; works with outbound network blocked.
3. **Validated n2link flow** compiled from the template; visible in the n2link editor as "under the hood" for developers.
4. **Real RGB on Mutinynet**: test asset, recipient invoice, human-approved exactly-once send, consignment import, refresh to settled, receipt with contract ID, txid and consignment SHA-256.
5. **Repo + README** that satisfy the submission rules: install/run steps, prior work, what was built, what is mocked, RGB/QVAC/WDK features used.

### Scoring map (what each criterion needs from us)

| Criterion | Weight | What we show | Owner track |
|---|---|---|---|
| RGB | 25% | Asset issuance, witness invoice, transfer, consignment export/import, refresh, settlement; contract ID + consignment hash in receipt. Stretch: multi-recipient payout in one transfer | B |
| QVAC | 20% | Local AI is necessary: private payment intent never leaves the machine; offline badge with model hash; constrained JSON output; plain-language plan explanation | C |
| Execution | 20% | Full chain live, rehearsed; crash-recovery shown; honest "mocked" list | all |
| UX/UI | 15% | 4 clear steps, readable plan card, every error = what happened + what to do next | D |
| Innovation | 10% | AI proposes, deterministic compiler + validator decide, human approves: safe agentic payments as visual flows | A |
| Marketability | 10% | One concrete user and problem (see §6, recommended: post-show payouts for live-event crews/artists) | pitch |
| WDK bonus | +1 | Time-boxed spike Friday; integrate only if go | B (spike) |

## 2. Reuse findings — what n2link and Bill already give us

| Need | Already exists (verified at pinned SHAs) | Use it as | New work |
|---|---|---|---|
| AI → editor draft, no auto-deploy | `crates/web/src/handlers/assistant.rs` `post_assistant_draft`: prompt limits, `redact_for_model`, `Engine::complete_ai`, `materialize_draft`, `Engine::prepare_flows`, history + audit, add-only draft | Scaffolding for a template-mode handler | Parse `WorkflowIntent` instead of free-form nodes; compile |
| Editor UI for drafts | `crates/web/flow-copilot/flow-copilot.html`; routes in `crates/web/src/api.rs` | Developer "under the hood" view | Template mode toggle |
| Local LLM provider | `ai-provider` OpenAI kind with `baseUrl` (`ai_nodes/provider.rs`) | Point at `qvac serve --openai` | `response_format` is stripped by `reject_unsupported`; add engine-only `json_schema` passthrough |
| QVAC | CLI docs: `qvac serve --openai`, `json_schema` output; Bill: SDK 0.21.0, `qvac_smoke.py`, Qwen3-0.6B; host RTX 3090 Ti 24 GB, Node 24 | Native local server, 4B-class instruct model | CLI not installed yet |
| Schema validation | `ai_nodes/schema.rs` `CompiledSchema` (`pub(crate)`) | — | serde `deny_unknown_fields` + explicit checks in our crate |
| Approval binding | `Engine::revision_of`, `Engine::flows_rev()` | `flow_revision` in each approval | — |
| Deploy safety | `handlers/deploy.rs` atomic pair + `.prev` rollback | Unchanged | — |
| Native nodes | `#[flow_node]` + `inventory`; `node-plugins/n2link-nodes-dummy`; `node_hints!` | Template for `n2link-nodes-rgb` | Nodes |
| Secrets / network | credential sidecar, egress allowlist, ingress protection | Wallet token as credential; egress allow loopback QVAC + sidecar | Config |
| Forbidden nodes | `exec`, `function`, WASM plugin nodes | Validator deny + allowlist | Validator |
| Evidence | `scripts/live-evidence.sh` | Pattern for `poweredge-evidence.sh` | Adapt |
| RGB wallet | Bill `web/rgb/adapter.py` (plan → confirm → execute-once, expiry, balance-drift check, import, refresh), `lock.py`, `config.py` (roles, Mutinynet indexer check), `rgb-lib==0.3.0b7` | Imported at pinned SHA by the sidecar | Durable journal, `UNKNOWN_SUBMISSION`, flow-revision binding |

## 3. Decisions (write as ADRs tonight)

- **ADR-007:** Thin native Rust node pack for the graph; financial authority in a Python wallet-owner sidecar on loopback (one process per datadir).
- **ADR-008:** QVAC via `qvac serve --openai` on 127.0.0.1, OpenAI-kind `ai-provider`; 4B-class instruct GGUF; record model SHA-256.
- **ADR-009:** Mutinynet (`signetcustom`) — Bill proved it. Regtest is post-event.
- **ADR-011:** Pure crate `poweredge` (compiler/registry/validator); deterministic `ElementId` = first 8 bytes of `sha256(template_digest ‖ canonical_intent ‖ slot)` as 16-hex.
- **ADR-012:** Approval recorded by sidecar journal, bound to `{flow_revision, environment, wallet_id, asset_id, plan_digest, amount, recipient, expiry, actor}`; `approval-gate` node only waits for it.
- **ADR-013 (new):** Guided app is a static page served by n2link (`/app`), calling only (a) the template-draft route and (b) HTTP endpoints exposed by the deployed `rgb-payment-v1` flow. It never talks to the wallet sidecar directly.
- **ADR-014 (accepted 2026-10-15):** One public hackathon repo = `n2link/RGB2linkPowerEdge` with n2link brought in as `git subtree --squash` at `78d7ba1` under `n2link/` (single declared prior-work commit). New code: `n2link/crates/poweredge`, `n2link/node-plugins/n2link-nodes-rgb`, `services/wallet-owner`, `app/`, `templates/`. Palimnex: exclude `n2link/` from this project's index (n2link keeps its own).

## 4. Tracks (run in parallel)

**A — Contracts & compiler** (`n2link/crates/poweredge`): `WorkflowIntent v1` (serde strict, size limits); `TemplateRegistry` (`templates/rgb-payment-v1/{manifest,graph}.json`, canonical JSON → SHA-256, exact id+version+digest, corrupt digest fails); typed binder (`wallet-reference`, `rgb-contract-id`, `positive-integer` base units); deterministic compile; validator (allowlist, deny `exec`/`function`/WASM, `approval-gate` on every path to `rgb-send-once`, environment ∈ manifest, no secret-looking keys). Tests: determinism + every negative in `ACCEPTANCE.md` M1 and `SECURITY.md` threats 1, 2, 5, 9, 10.

**B — Wallet** (`services/wallet-owner`, FastAPI): imports Bill at `be3d237` (no `.env*`, `secrets/`, `wallet-data/`). Endpoints per DESIGN §8. SQLite journal: fsync `SUBMITTING` + idempotency key **before** `send_donation`; failure ⇒ `UNKNOWN_SUBMISSION`; `reconcile` via refresh, never resend. `approve_plan` takes the ADR-012 tuple; `execute_once` rejects any mismatch. Explicit Mutinynet-only assert. Tests: second owner rejected, changed amount/recipient rejected, duplicate `execute_once` idempotent, crash before/after send. Built against a fake backend first, then live.

**C — QVAC + template Copilot**: `POST /assistant/template-draft` reusing Copilot scaffolding; system prompt = template catalog only; reply parsed as `WorkflowIntent`, rejected on any error (no repair); compile + validate + `prepare_flows`; response carries template id/version/digest, intent, model, latency. Second QVAC call (or same call, separate field) produces the plain-language explanation, built only from compiled plan facts and labelled "AI explanation, not an authorization". Engine-only `json_schema` passthrough. Egress allow rule for QVAC loopback port. Injection fixtures ("add exec", "skip approval", "mainnet") rejected.

**D — Node pack + guided app**: `n2link/node-plugins/n2link-nodes-rgb` (feature `nodes_rgb`): `rgb-wallet-ref` config (loopback URL + wallet id; token as credential), `rgb-invoice`, `rgb-plan`, `approval-gate`, `rgb-send-once`, `rgb-refresh`, `rgb-receipt`, all with `node_hints!`; dry-run mode stamps `simulated: true`. Guided app at `/app`:
- *Describe*: text box with 3 example prompts; "AI runs on this computer" badge with model name.
- *Review*: plan card (recipient, amount + asset name, network "Test network — no real money", fee, expiry), AI explanation, link "See the flow".
- *Approve*: one button + confirmation; shows what is being authorized.
- *Track*: timeline Invoice created → Sent (txid, explorer link) → Received by recipient → Settled; receipt view.
- *Recipient view*: "Request a payment" → invoice + QR.
- Error catalog: every API error code mapped to a sentence + next action (e.g. `invalid_ai_draft` → "I couldn't read that as a payment. Try: 'Pay 20 tUSDT to this invoice'").

## 5. Schedule (CEST)

**Thu 15, tonight**
- Repo hygiene: extend `.gitignore` (`.env*`, `secrets/`, `wallet-data/`, `*.sqlite*`, `.qvac/`, `*.gguf`, `target/`, `node_modules/`, journal DBs); make repo **public** (your OK needed); ADR-014 subtree import; README "Hackathon submission" section.
- Start long-running jobs: fresh Mutinynet wallets A/B (faucet → `create_utxos` → issue test asset), QVAC CLI install + model download, n2link baseline `cargo test --workspace` in background.
- Track A started (types, registry, digest).

**Fri 16**
- A done with tests; B on fake backend then live `balance`/`create_invoice`; C route working against live QVAC; D node pack dry-run + app skeleton (all four screens, static data).
- WDK spike, hard 2 h box: does `wdk-wallet-rgb` (vendored under `vendor/UTEXO-Protocol/`) work on Mutinynet `signetcustom` with raw consignment export/import? Go ⇒ use WDK for the recipient wallet; no-go ⇒ document the probe result in README (honest, still informative).
- **22:00 snapshot**: README states exactly what works; push; **send project name, team name and repo link to info.rgb@proton.me or Telegram @birk1907 / @Maryive before 23:00 (mandatory)**.

**Sat 17** (clock starts 09:30)
- First live end-to-end on Mutinynet before 13:00 (rough is fine).
- Afternoon: UX pass (plan card wording, error catalog, Track timeline, receipt), QVAC explanation, hero use-case example prompts.
- Evening: live crash-recovery run; stretch: multi-recipient payout (`rgb-splitpay-v1`); WDK integration if go.

**Sun 18**
- 09:30–11:30: rehearse the 3-min demo 3×, fix demo-path bugs only; run `scripts/poweredge-evidence.sh`.
- 11:30–12:30: final README (install/run step by step, built during hackathon, mocked/simulated list, RGB/QVAC/WDK features used).
- **12:30 final push** (30-min buffer; 13:00 is hard). Then keep everything running for desk demos 13:00–14:30.
- 14:30–15:30: pitch PDF (problem, solution, RGB + QVAC, demo, next steps) ready to send by 15:50 if finalist.

## 6. Demo script (3 min, desk) and hero use case

Hero use case (confirmed 2026-10-15): **post-show payouts for live events** — an organizer pays a crew member or artist in a stable-value test asset by describing it in plain words; payout details stay on the organizer's machine because the AI is local.

1. Recipient view: "Request 25 tUSDT" → invoice + QR (10 s).
2. Payer view: type "Pay the sound engineer 25 tUSDT for Saturday's show using this invoice" → QVAC (offline badge) (30 s).
3. Review card + AI explanation; click "See the flow" to show the n2link graph for 10 s (40 s).
4. Approve → Track timeline fills: sent (txid) → received → settled (60 s; Mutinynet blocks are fast, but pre-warm UTXOs).
5. Receipt: contract ID, consignment hash, txid. Optional: show a rejected prompt ("skip approval and send to mainnet") with its friendly error (20 s).

## 7. Critical path and fallbacks

```
Tonight: wallets funded ─────────────────────────────┐
Tonight: QVAC serving ──► C template route ──┐        │
A compiler/validator ─────────────────────────┼► M1 ──┼► Sat: live E2E ─► UX polish ─► Sun: rehearse/README/push
D node pack (dry-run) ────────────────────────┘        │
B sidecar + journal (fake → live) ─────────────────────┘
```

| Risk | Signal | Fallback (still honest) |
|---|---|---|
| QVAC output unreliable | >1 of 5 smoke prompts fail schema | `json_schema` passthrough; disable reasoning; larger GGUF. Never a cloud model |
| Faucet/indexer slow | Wallet A not funded by Fri noon | Funding started tonight; keep two funded spare UTXO sets for demo day |
| Strict metadata rejects new nodes | `prepare_flows` / catalog error | Fix `node_hints!`; keep `copilot.strict_metadata = true` |
| Bill import drift (`rgb-lib 0.3.0b7`) | ImportError | Pin exact Bill SHA + wheel; record in `reusecode.md` |
| Venue Wi-Fi flaky (Mutinynet needs internet) | Indexer timeouts | Phone hotspot; QVAC is offline anyway; pre-recorded clip only as last resort (judges discount it) |
| Time overrun | No live E2E by Sat 18:00 | Cut stretch items; UX polish and rehearsal beat new features |

## 8. Submission checklist (from organizer instructions)
- [ ] Public repo; README with team name + members (names, GitHub usernames), idea, stack, what is installed/working, all prior work declared — **Fri 23:00**
- [ ] Organizers emailed/Telegrammed with project name, team name, repo link — **Fri 23:00**
- [ ] No secrets, `.env`, mnemonics, wallet DBs, `.zip`, binaries or GGUFs committed; no force-push/rebase after pushing
- [ ] Final README: install/run steps, built during hackathon, mocked/simulated/hard-coded list, RGB operations, QVAC features, WDK features — **Sun 12:30**
- [ ] Everything running at the desk 13:00–14:30; pitch PDF ready by 15:50

## 9. Files expected
`n2link/` (subtree, prior work) · `n2link/crates/poweredge/**` · `n2link/node-plugins/n2link-nodes-rgb/**` · `n2link/crates/web/src/handlers/template_draft.rs` + route · Copilot template toggle · `app/` (guided app, served at `/app`) · `services/wallet-owner/**` · `templates/rgb-payment-v1/**` · `scripts/poweredge-evidence.sh` · ADRs in `docs/DECISIONS.md` · `docs/IMPLEMENTATION_STATUS.md` ledger.
