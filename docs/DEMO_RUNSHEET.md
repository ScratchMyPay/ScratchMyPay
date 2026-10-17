# Demo run-sheet (rehearsed Saturday Oct 17, all keys in micro-VMs)

Timings are from automated rehearsals on Mutinynet (`scripts/e2e/app-payout.mjs`, `scripts/e2e/rehearse-crash.sh`).
Block times vary (~30 s); treat them as typical, not guaranteed.

## Before going on stage (5 min)
1. `scripts/demo-up.sh status`: the first line must read
   `Keys: organizer wallet in a micro-VM …; crew wallet in a micro-VM …; WDK treasury in a micro-VM …`,
   all five services `ready`, organizer USDT/TUSDT and crew balances shown, treasury `OK`.
2. `scripts/vm/smoke.sh` → `SMOKE PASS` (~1.5 s).
3. Open the app on the stage laptop (`/poweredge-app/login`), buy the door pass once, keep the tab open.
4. Do **not** restart n2link while the stage browser is open (passes are tied to that session).
5. The app's **Keys in a box** strip shows three green boxes; the editor tab **Keys in a box** shows three green dots and "3/3 boxes running · keys in VMs".
6. Automation note: the door's demo button pays at most once per 30 s; wait 30 s between scripted runs.
7. Optional second screen: **`/poweredge-app/dashboard`** ("Dashboard" in the app header). During the ~90 s of Track it shows the day so far: payouts per hour, p50/p90 approve → settled, door revenue, treasury, key boxes; it updates every 30 s.

## Segment timings (all keys in VMs)
| Step | What the audience sees | Typical time |
|---|---|---|
| Door | pay 1,000 test sats, pass issued (organizer VM pays treasury VM) | 9–10 s |
| Crew | crew tab: payment request with the KaleidoSwap proxy | ~1 s |
| Describe → Review | plain sentence → local AI (QVAC) draft → review | 3–4 s |
| Approve | typed name + checkbox | (as long as you talk) |
| Track → SETTLED | prepared ✓ approved ✓ sending ✓ sent ✓ delivered ✓ settled ✓ | 80–95 s after approve |

Fill the ~90 s of Track with the architecture slide: the AI on the host, every key in its own VM, one socket out.

## The keys-in-a-box moment (crash), ~2 min
Run **before** the payout you will crash: `POWEREDGE_CRASH_AT=after_broadcast scripts/vm/boot.sh up payer`
(after `scripts/vm/boot.sh down payer`; ~40 s). `scripts/e2e/rehearse-crash.sh` does the whole moment unattended.
| t (after approve) | Screen | Say |
|---|---|---|
| ~15 s | "Sending on Bitcoin…"; the organizer VM exits right after sending; **Keys strip: organizer box turns red** ("VM down — key stays on its disk") | "The box holding the organizer's key just died, after the money left, before it wrote that down." |
| — | run `scripts/vm/boot.sh up payer` (~40 s: the wallet syncs) | "We boot a fresh box from the same disk." |
| ~60 s | organizer box green again (new pid); "We can't confirm yet whether it was sent. It will NOT be sent twice." (prepared ✓ approved ✓ kept) | "It refuses to guess. It never sends twice." |
| ~75 s | sent ✓ delivered ✓: the wallet reconciled from its own record, by itself | |
| ~125 s | settled ✓, receipt; crew balance +1 | "Paid exactly once." |

## Fallback (no VMs), if anything VM-related misbehaves
```bash
scripts/demo-up.sh stop
for r in payer recipient wdk; do scripts/vm/boot.sh return $r; done
scripts/demo-up.sh up        # ~75 s; status says "host process"; payout settles in ~50 s
```
Back into the VMs afterwards: `scripts/demo-up.sh stop`, then `scripts/vm/boot.sh up recipient|wdk|payer`, then `scripts/demo-up.sh up` (~3 min).

## Fixed during rehearsal (Oct 17)
- After a crash the Track screen waited forever: the organizer wallet now finishes in-flight payouts itself (20 s background check; reconcile never sends).
- The timeline dropped prepared/approved ticks while unknown: fixed.
- A VM shut down mid-refresh left rgb-lib's runtime lock; the host fallback then hung at start. The service now waits for the operation in progress before exiting, and clears the lock at start only when no other process owns the wallet.


## Flow Copilot segment: "pay me" page (QVAC, ~1 min)

1. **Reload the editor (F5) first.** An editor tab left open deploys its old copy of the flows and can undo changes made elsewhere (seen 19:44 on Oct 17).
2. New empty tab → Flow Copilot sidebar → *"Make a web page at /pay-bassist that creates an RGB payment request and shows it as a QR code."*
3. QVAC drafts 4 nodes (`http in` → `rgb-invoice` → `qr-code` → `http response`, ~9 s). n2link gives the `http in` host 127.0.0.1 and a free port; the sidebar shows **"Endpoints (after Deploy)"** with the exact link (e.g. `GET http://127.0.0.1:1896/pay-bassist`; if a port was taken it says which node holds it).
4. Apply → Deploy (~0.3 s) → click the link: QR of a real Mutinynet RGB invoice from the crew wallet.
5. What to say: *"Copilot can build anything that asks for money, never anything that sends it: sending only goes through the pinned template and a human approval."*
6. Fallback prompt: *"Give the drummer a pay-me link."* To move an existing `http in` (host/port are not in the stock form): `cd scripts/e2e && node set-http-port.mjs --url /path --port 1897`.

## Before the show: door demo button needs plain sats

The organizer's plain (non-RGB) sats fund the door's demo button (1,199 sats per pass incl. fee). On Oct 17 at 19:05 only 711 were left. Top up from the Mutinynet faucet to the organizer address `tb1pncw03gkwd65gydgq2pq499xq09nsdymhp6eara5cq9de8m9mjdzs3dd26s`. RGB payouts are not affected (they are paid from RGB outputs). Scripts use the operator login and do not buy passes.
