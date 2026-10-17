# SECURITY — mandatory boundaries

## Threat model
Adversaries: malicious or accidental user prompt, poisoned template, hostile GitHub/web evidence, editor user without signer rights, buggy Wasm node, compromised API credential, ambiguous broadcast, wrong network, service capsule breakout, wallet datastore race.

| Boundary | Enforced mechanism | Must fail closed on |
|---|---|---|
| QVAC -> compiler | JSON schema, allowlisted template IDs | Arbitrary code, node types, permissions |
| Compiler -> deploy | Deterministic IR, graph validation, revision-bound approval | Bypassing approval gates, mutable parameters |
| Runtime -> wallet | Narrow typed RPC, explicit scopes | Direct seed/RPC access, unsigned intents |
| Wallet -> chain | Single datadir owner, durable idempotency/reconciliation | Repeat send when outcome unknown |
| Capsules -> host | Binary/image allowlist, isolated volumes, user/network/resource policy | Arbitrary shell commands and public RPC binding |
| Receipts -> audience | Signed digest and explicit verification status | False claim that AI judgment equals RGB validity |

- Default to test-only networks; mainnet disabled by build/config and manual per-action controls if later enabled.
- QVAC never holds spending authority. Do not put wallet seeds or private keys in Node-RED `msg`, function code, models, logs or demo videos.
- No direct arbitrary API-to-signer path. `approval_id` ties plan digest, flow SHA, signers, wallet/network, amount/recipient, expiry and actor.
- Idempotency IDs are unique per intent; journal writes precede irreversible external operations.
- Never interpret CRC32, RSSI or transport ACK as payment authenticity or settlement.
- Design `UNKNOWN_SUBMISSION` recovery without blind retries. Network errors do not imply no broadcast.
- Encryption-at-rest is not equivalent to a protected signer; document actual deployment key protection.
- Verify existing n2link credential, egress, ingress and Wasmi boundaries before changing them; retain upstream regression tests.

## Keys in a box: what the AI can and cannot reach (ADR-027)

Everything the AI touches runs on the host; every key runs in its own Firecracker micro-VM with no network card.

| Component | Runs where | Can reach | Cannot reach |
|---|---|---|---|
| QVAC (local model), Copilot, flow editor, n2link runtime, guided app | host | the wallets' HTTP APIs through `127.0.0.1:8711-8713` (bearer token; plan / approve / execute once) | any seed, any wallet data dir, the chain directly |
| Organizer wallet (rgb-lib via Bill) | micro-VM `payer` | mutinynet.com:443, proxy.kaleidoswap.com:443, the crew VM (delivery) | anything else: no network card, only these vsock pinholes |
| Crew wallet | micro-VM `recipient` | mutinynet.com:443, proxy.kaleidoswap.com:443 | the organizer VM, the host, anything else |
| WDK treasury (and the door) | micro-VM `wdk` | mutinynet.com:443 (fees), electrum.mutinynet.com:50002 | anything else |

- A seed exists on the host only in `secrets/` (git-ignored, 0600). `boot.sh` copies it into a read-only seed disk in a private tmp dir and unlinks that file as soon as the VM has it open; no host process keeps `secrets/` open or carries a mnemonic in its arguments (checked live, `docs/IMPLEMENTATION_STATUS.md` 2026-10-17 15:50).
- One live copy per wallet: while `datadirs/<role>.IN_VM` exists, `run.sh` and the WDK server refuse to start on the host.
- Not claimed: a compromised host can still read `secrets/` and the VM disk images. The VM boundary protects the keys from the AI-facing processes and from the network, not from root on the same laptop; a real deployment puts each VM on its own machine or the seed in a hardware signer.

## Threat tests
1. QVAC output injects arbitrary `exec` node or exposed RPC: rejection.
2. Flow edits amount after approval: approval invalid.
3. Payment broadcast succeeds but response lost: exactly one authorized send, pending reconciliation.
4. Restart before/after journal commit: no duplicated transaction intent.
5. Wrong RGB network tag or schema: rejected.
6. Duplicate invoice/consignment: idempotent import or safe rejection.
7. Organizer VM dies right after broadcast, new VM boots on the same disk: exactly one send, reconciled, settled (`scripts/crash_recovery_demo.py crash|recover --vm`, PASS 2026-10-17).
7. Capsule manifest public-bind RPC/host mount/privileged: rejected.
8. Unauthenticated webhook invokes privileged action: denied.
9. Signing secret included in model input or emitted output: rejected/redacted.
10. Corrupt template digest: fails to load.
