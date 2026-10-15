# Architecture decisions and open questions

| ID | Status | Decision |
|---|---|---|
| ADR-001 | Accepted design | Extend n2link; no duplicated new flow engine. |
| ADR-002 | Accepted design | QVAC selects parameterized, pinned templates; never arbitrary privileged flow generation. |
| ADR-003 | Accepted design | AI/planner/runtime separated from financial signer and journal. |
| ADR-004 | Accepted design | Native process and OCI capsules, not bitcoind in Wasmi; Firecracker deferred. |
| ADR-005 | Accepted design | rgb-lib-python from Bill is first real wallet backend; WDK optional independently owned wallet. |
| ADR-006 | Accepted design | Visual RGB dApps, not unrestricted on-chain smart contract compilation. |
| ADR-007 | Open | n2link extension point: native node pack vs separate process with service nodes; inspect current code. |
| ADR-008 | Open | Local QVAC installed version, CPU/GPU capability, structured-output implementation. |
| ADR-009 | Open | Regtest full RGB compatibility versus existing tested Mutinynet. Prefer first verified end-to-end path. |
| ADR-010 | Open | Accepted escrow script policy and test fixture design for MergePay (later). |

Record implementation findings as new ADRs rather than silently overriding these choices. All stated components are proposed until implemented and tested.
