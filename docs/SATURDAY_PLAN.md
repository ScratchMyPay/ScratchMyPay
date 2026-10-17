# Saturday plan — ScratchMyPay — "Keys in a box": wallets in Rust micro-VMs

**Date:** Saturday 2026-10-17 (hackathon day 1, clock starts 09:30).
**Goal in one sentence:** *the AI drafts, a human approves, and the key lives in a box the AI cannot reach.*
Each wallet seed runs inside its own Firecracker micro-VM (Firecracker is written in Rust); n2link, QVAC and
the Copilot-built flows stay on the host and reach a wallet only through one vsock socket.

This is the work the judges will see in the Friday → Sunday diff. It touches no money-path logic
(journal, approval, send-once stay exactly as they are); it moves where the wallet services *run*.

## What we build (scope)

| # | Deliverable | Done when |
|---|---|---|
| 1 | **Keys in a box** — organizer wallet, crew wallet and WDK treasury each in a micro-VM | `demo-up.sh` boots three VMs; a payout from the guided app settles end to end; `grep -r` for a seed on the host finds nothing outside `secrets/` (which the VMs mount read-only and the host services never open) |
| 3 | **One crew member = one VM** — consignment delivery crosses a real VM boundary | Organizer VM delivers the consignment to the crew VM over vsock, not loopback; README "mocked" list loses the "both wallets on one laptop" line |
| 4 | **Crash demo, VM edition** — destroy the organizer VM mid-payout, boot a fresh one, still paid exactly once | `crash_recovery_demo.py --vm` passes: journal on a persistent disk survives; UNKNOWN_SUBMISSION → reconcile → SETTLED, one submission |

**Not in scope:** QVAC in a VM (GPU), sandboxing Copilot flows in VMs (the deploy guard already limits them),
judge-bootable images (binaries can't be committed), any change to the payout journal or approval logic.

## Architecture

```
HOST (Linux, KVM)                                   micro-VMs (Firecracker, Rust)
┌──────────────────────────────────┐    vsock 8711  ┌───────────────────────────┐
│ n2link (flows, Copilot, app)     │◄──────────────►│ VM "organizer"            │
│ QVAC (local AI, GPU)             │                │  wallet-owner payer       │
│ wdk-proxy? no: WDK is its own VM │    vsock 8713  │  /secrets/payer.mnemonic  │
│ demo-up.sh (boots VMs)           │◄──────────────►│  /data (journal, datadir) │
└──────────────────────────────────┘                └──────────┬────────────────┘
                                                               │ vsock 8712 (deliver)
                                                    ┌──────────▼────────────────┐
                                                    │ VM "crew"                 │
                                                    │  wallet-owner recipient   │
                                                    └───────────────────────────┘
                                                    ┌───────────────────────────┐
                                                    │ VM "treasury" (WDK, node) │
                                                    └───────────────────────────┘
```

- **One socket per wallet.** The host talks to a VM only through its vsock port. The HTTP API inside the VM
  is unchanged (same bearer token, same routes), so n2link's `rgb-wallet-ref` and `wdk-wallet-ref` nodes
  keep working; only their `baseUrl` changes to a host-side vsock bridge on 127.0.0.1.
- **Network:** each VM gets a tap interface with NAT only to the Mutinynet indexer (`mutinynet.com`) and
  Electrum (WDK). The host firewall drops everything else from the tap. The VMs never see each other
  except organizer → crew on vsock (consignment delivery).
- **Disks:** one shared read-only rootfs (Debian + Python 3.12 + rgb-lib 0.3.0b7 + Node 22 for WDK), one
  small persistent ext4 disk per VM for `datadirs/` and the journal. Seeds are injected at boot through a
  tiny read-only disk built from `secrets/` (never through the kernel command line or the API, which show up in
  process lists).
- **Crash = destroy the VM.** `crash_recovery_demo.py --vm` sends `POWEREDGE_CRASH_AT=after_broadcast` as
  before, but the service exits by killing the VM process from inside; demo-up boots a new VM on the same
  persistent disk.

## Schedule (CEST)

| Time | Step | Owner | Output |
|---|---|---|---|
| 09:30–09:45 | **Go/no-go gate** (below) | Gus + Claude | decision in `IMPLEMENTATION_STATUS.md` |
| 09:45–10:45 | Install Firecracker (release binary, pinned version, checked SHA) + download Firecracker's CI kernel; `scripts/vm/setup.sh` | Claude | `firecracker --version`; kernel boots a stock rootfs |
| 10:45–12:15 | Build the rootfs: `scripts/vm/build-rootfs.sh` (debootstrap or docker export), Python venv with Bill + wallet-owner installed, Node 22 + wdk-btc; `vsock-bridge` (small Python: host TCP 127.0.0.1:871x ↔ vsock) | Claude | organizer VM answers `/health` over the bridge |
| 12:15–13:00 | Seeds disk + persistent data disk; boot organizer VM with the real payer datadir; `GET /funds` works | Claude | funds report from inside the VM |
| 13:00–13:45 | Lunch / cms review of the diff so far | | |
| 13:45–14:45 | Crew VM + organizer→crew delivery over vsock (`PEER_URL` → vsock bridge inside the VM) | Claude | crew VM imports a consignment |
| 14:45–15:30 | Treasury VM (WDK); `demo-up.sh` gains `--vm` mode (boot order, readiness over the bridges, `stop` destroys VMs) | Claude | `demo-up.sh status --vm` all ready |
| 15:30–16:15 | **Live payout through the VMs** from the guided app (1 TUSDT) | Gus drives | txid in `artifacts/payouts/` |
| 16:15–17:00 | Crash demo VM edition: `crash_recovery_demo.py --vm` | Claude | PASS evidence file |
| 17:00–17:45 | README (architecture diagram, mocked list update, how to run), ADR-025, `SECURITY.md` section "what the AI can and cannot reach" | Claude | docs |
| 17:45–18:00 | Commit + push; `demo-up.sh` without `--vm` still works (fallback for Sunday) | | |
| Evening | Rehearse the 3-minute demo twice **with** VMs, once without | Gus + cms | |

Total build: ~7 h with lunch; the schedule has an hour of slack before the evening.

## Go/no-go gate (09:30)

Go only if **all** are true; otherwise fall back to the Wasm-sandbox story (below) and spend Saturday on splitpay + the QVAC-reason work.

1. The laptop going to the venue is Linux with `/dev/kvm` and the user in the `kvm` group (`ls -la /dev/kvm`, `grep -m1 -E 'vmx|svm' /proc/cpuinfo`). andorxps passes; **the Turin laptop is unknown as of Friday night — check it Friday evening.**
2. Firecracker's release binary and CI kernel download fine on the venue wifi (or are cached on the laptop Friday night: `scripts/vm/setup.sh --prefetch`).
3. Rootfs build finishes in under 45 min on the laptop (debootstrap needs network; prefetch the `.deb` cache Friday if possible).
4. Friday's snapshot is pushed and the organizer message is sent (hard requirement regardless).

## Acceptance tests

- [x] `scripts/vm/smoke.sh`: boots one VM, hits `/health` over the bridge, destroys it. Under 30 s. **PASS 1.3–1.4 s.**
- [x] From the host: `ps aux | grep -c mnemonic` is 0 and no host process has `secrets/` open (`lsof +D secrets` empty while a payout runs). **Checked 15:50, all three keys in VMs.**
- [x] Full screen-driven payout settles through the VMs; journal shows the normal chain of states. **Via the KaleidoSwap proxy and VM → VM; `artifacts/payouts/2026-10-17-vm-*.json`.**
- [x] `crash_recovery_demo.py --vm` PASS; evidence lists the destroyed VM's id and the new one's. **Also through the app: `scripts/e2e/rehearse-crash.sh`.**
- [x] `demo-up.sh stop --vm` leaves no `firecracker` processes and no tap devices. **`demo-up.sh stop` (VM-aware, no flag needed); VMs have no tap at all.**
- [x] `demo-up.sh` (no `--vm`) still works unchanged: this is Sunday's fallback if anything in the VM path misbehaves. **Rehearsed: `boot.sh return` all three → host payout 48 s → back.**
- [x] n2link tests 75/75, wallet-owner 26/26, clippy clean (the VM work should not touch Rust code except `baseUrl` defaults). **Now: n2link-web 86/86 + vmbox 3/3, wallet-owner 44/44, wdk-btc 11/11, clippy clean; Rust gained the read-only `vm-status` node by decision.**

## Fallbacks

| If… | Then… |
|---|---|
| No KVM on the venue laptop | **Wasm sandbox story instead:** Copilot-built `function` nodes run in n2link's Wasm plugin sandbox with no filesystem or network; same pitch sentence ("the AI's code runs in a box"), weaker on keys. ~3 h. |
| Rootfs build fights us past 12:30 | Switch to a prebuilt Debian cloud image + `apt install` inside the VM; accept a larger disk. |
| vsock bridge flaky | Use tap networking only: VM API on `172.16.x.x:871x`, host firewall allows only the host → VM port. Lose nothing in the pitch. |
| Crash demo VM edition fails to reconcile | Keep the process-level crash demo (already PASS) and say so honestly. |
| Over time at 16:15 | Ship 1 only (organizer VM), keep crew on the host, state it in the mocked list. |

## Pitch sentence and demo moment

> "Everything the AI touches runs on the host. Every key runs in a Firecracker micro-VM with one socket out. When we destroy the organizer's VM in the middle of a payout and boot a new one, the crew still gets paid exactly once."

Demo moment (30 s): `demo-up.sh status --vm` shows three VMs → payout from the app → `pkill` the organizer VM at "sending" → new VM boots → Track screen goes to SETTLED, crew balance +1.

## Files expected

```
scripts/vm/setup.sh            # pinned Firecracker + kernel download, SHA check, --prefetch
scripts/vm/build-rootfs.sh     # reproducible rootfs (no image committed)
scripts/vm/boot.sh             # boot one VM: role, seeds disk, data disk, vsock port, tap
scripts/vm/vsock-bridge.py     # host 127.0.0.1:871x <-> vsock
scripts/vm/smoke.sh
scripts/demo-up.sh             # --vm mode
scripts/crash_recovery_demo.py # --vm
docs/DECISIONS.md              # ADR-025 keys in a box
docs/SECURITY.md               # reach table: host vs VM
README.md                      # architecture, run steps, mocked list
artifacts/payouts/2026-10-17-vm-payout.json, 2026-10-17-vm-crash-recovery.json
```
