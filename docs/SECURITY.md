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

## Threat tests
1. QVAC output injects arbitrary `exec` node or exposed RPC: rejection.
2. Flow edits amount after approval: approval invalid.
3. Payment broadcast succeeds but response lost: exactly one authorized send, pending reconciliation.
4. Restart before/after journal commit: no duplicated transaction intent.
5. Wrong RGB network tag or schema: rejected.
6. Duplicate invoice/consignment: idempotent import or safe rejection.
7. Capsule manifest public-bind RPC/host mount/privileged: rejected.
8. Unauthenticated webhook invokes privileged action: denied.
9. Signing secret included in model input or emitted output: rejected/redacted.
10. Corrupt template digest: fails to load.
