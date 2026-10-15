# ACCEPTANCE — executable gates

## M0
- `cargo test --workspace` or appropriate repository-targeted baseline; document missing environment prerequisites and exact commands rather than claiming success.
- `cargo fmt --check`; targeted clippy where feasible. Record base SHA and current feature flags.

## M1 — required
- Valid `WorkflowIntent` conforms to JSON schema; malicious/unbounded/missing fields rejected.
- Registry resolves exact pinned ID/version/hash; no fallback to nearest version.
- Compiler determinism: hash canonical compiled IR twice; match.
- Required approval is present in graph; removing/bypassing it produces validation error.
- No unsafe arbitrary shell, dynamic JS, hidden credentials or wildcard capsule operations from QVAC-generated graph.
- Local QVAC inference: verify process/model actually ran locally; no mocked result presented as real.
- Generated n2link flow accepted by current parser and rendered in editor; absent node must fail explicitly.
- Preview then explicit authenticated deploy; changed flow rev invalidates approval.

## M2 — real RGB end-to-end
- Correct network and asset schema, real invoice, immutable plan and one-use confirmation.
- Actual Bitcoin txid, real RGB consignment, recipient import, refreshed settled balance; capture timestamps and hashes.
- Kill process immediately before and after external send; recover without blindly retrying.
- Wallet-owner concurrency test rejects second owner/process on same datadir.
- Wrong-network transfer fails closed.

## M3 — capsules
- Only allowed binary/image/ports/volume; endpoint private by default.
- Provision/start/ready/stop/delete cycle works twice without collisions.
- Crash/restart preserves declared state and cleans orphan resources.
- Deny a wildcard bind, host root mount and arbitrary executable.

## Submission demonstration
Record separate screenshots/logs for: prompt; QVAC actual structured response; selected template ID/digest; validated graph; editor; plan/approval; RGB tx and asset delta; recovery evidence; dated commit list. Label simulated values. Do not disclose wallet secrets.
