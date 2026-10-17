# Dashboard plan — ScratchMyPay "show-night dashboard" (proposal, Oct 17 evening)

**Goal:** a live dashboard built **as an n2link flow** (dashboard nodes on a tab, Copilot can build/adapt it), showing
the money and the machinery of the demo: sats and dollars in each wallet, payouts per hour, door revenue, fees,
time-to-settle, and the key boxes. Read-only: no dashboard node can move money or touch a VM.

## 1. Options evaluated

| Option | How | Verdict |
|---|---|---|
| A. Install Node-RED Dashboard 2.0 (`@flowfuse/node-red-dashboard`) | npm package of JS/Vue nodes | **Not possible.** n2link is a Rust runtime: JS node packages do not run in it, there is no npm/palette installer, and dashboard widgets are a declared non-goal (`n2link/adoption/phase6/DESIGN.md`). Running a real Node-RED next to n2link would mean a second runtime, a second login and a second network surface. |
| B. Dashboard from existing nodes only | `http in` → `template` (HTML + JS in one node) → `http response`; data pushed with `websocket out` | Possible, zero Rust, but a 300-line HTML page inside a template node is hard to edit or for Copilot to adapt, the websocket endpoint needs its own auth story, and history is lost on reload. Good as a 30-min fallback. |
| C. **Small native dashboard nodes + one dashboard page (recommended)** | Rust widget nodes (`ui-stat`, `ui-chart`, `ui-table`) keep their latest value and a short history in n2link; one page behind the door renders them | Native, door-authenticated, history survives a page reload, each widget is one node on the canvas (Copilot-friendly), works offline (charts drawn as inline SVG, no CDN). Generic: useful to n2link beyond this hackathon. |
| D. Guided-app page only (no nodes) | the app calls the wallet/WDK APIs directly | Fastest (~1.5 h) but not a flow: misses the point "the dashboard is built in n2link". |

## 2. Recommended architecture (C)

```
inject (every 10 s / 60 s)
  → data nodes (read-only)                → widget nodes                     → page
    rgb-funds  (organizer, crew)            ui-stat  "Organizer sats"          /poweredge-app/dashboard
    payout-stats (per hour, p50/p90)        ui-chart "Payouts per hour"        (behind the door pass;
    wdk-balance (exists), door-stats        ui-table "Last payouts"             polls /poweredge-app/api/dashboard
    vm-status (exists)                      ui-stat  "Key boxes 3/3"            every 3 s; EN/ES; phone-friendly)
```

- **Widget nodes** store `{title, group, order, unit, value, history[≤288], updatedAt}` in an in-memory registry inside
  n2link (process lifetime; a restart re-fills within one tick). Status dot on the canvas shows the last value.
- **Page** lays widgets out by `group` (Wallets · Payouts · Door & treasury · Keys) and `order`; inline SVG line/bar
  charts; same look as the app (Satoshi palette). Reachable from the app header ("Dashboard") and on the stage screen.
- **Data nodes** use the existing wallet/WDK config nodes (token handling already solved), never write.

## 3. What we can show (all real data from the demo)

| Group | Widget | Source |
|---|---|---|
| Wallets | Organizer: sats (plain + on RGB outputs), USDT, TUSDT spendable, "payouts possible" | wallet `/funds`, `/assets` |
| Wallets | Crew: USDT/TUSDT received | crew `/assets` |
| Payouts | Payouts per hour (settled / failed), last 24 h | journal (new `/stats`) |
| Payouts | Volume per hour per asset (USDT, TUSDT) | journal |
| Payouts | Time from approval to settled, **p50 / p90** | journal events (APPROVED → SETTLED) |
| Payouts | Last 10 payouts: amount, to, state, txid (link) | journal (new `/plans?limit=10`) |
| Door & treasury | Treasury total sats (main + door), door revenue, passes per hour, active passes | WDK `/balance`, `/door/passes` |
| Door & treasury | Network cost per payout now (sat) | `wdk-fee-quote` (exists) |
| Keys | Key boxes running 3/3, uptime | `vm-status` (exists) |
| (stretch) | Outbound connections per minute (egress observe) | needs a counter endpoint in n2link |

## 4. Work items and estimates

| # | Item | Where | Est. |
|---|---|---|---|
| 1 | Wallet API: `GET /stats?hours=24` (per-hour counts/volume, p50/p90 settle time) and `GET /plans?limit=10`; tests | `services/wallet-owner` (+ VM image rebuild, clean organizer VM restart ~40 s) | 40 min |
| 2 | Data nodes `rgb-funds`, `payout-stats` (on `rgb-wallet-ref`), `door-stats` (on `wdk-wallet-ref`); editor HTML | n2link `rgb_nodes` | 45 min |
| 3 | Widget nodes `ui-stat`, `ui-chart`, `ui-table` + registry + `GET /poweredge-app/api/dashboard`; editor HTML; tests | n2link core + web | 75 min |
| 4 | Dashboard page (SVG charts, groups, EN/ES, phone layout) + "Dashboard" link in the app | `poweredge-app/dashboard.html` | 45 min |
| 5 | Flow Library seed "Show-night dashboard" + deployed tab + screenshots + docs | library, scripts/e2e | 30 min |

**Total ≈ 4 h.** Split into two phases so tonight stays safe:

- **Phase 1 — MVP (~2 h, items 3, 4 and part of 2):** `ui-stat` + `ui-chart` + page, fed by nodes we already have
  (`wdk-balance`, `vm-status`) plus `rgb-funds`. Shows: wallet sats/USDT, treasury and door revenue, key boxes,
  and charts that build up while the dashboard runs.
- **Phase 2 — history (~1.5–2 h, items 1, rest of 2, `ui-table`, 5):** payouts per hour, volume, p50/p90
  time-to-settle, last payouts table, passes per hour — from the journal and the door DB, so the charts show the
  whole day immediately.

## 5. Risks and how we contain them

| Risk | Mitigation |
|---|---|
| Rust build/restart time (3–5 min each) eats the evening | batch changes, one build per phase; restart n2link only between rehearsals |
| Touching the money services the night before | read-only endpoints only; wallet-owner change is additive and tested; organizer VM restart is the clean path rehearsed today |
| Polling load on Mutinynet's indexer | dashboard reads local journal/DB data; wallet `/funds` at most once per 60 s |
| Stage Wi-Fi without internet | no CDN: charts are inline SVG, page is served by n2link |
| Scope creep vs. Sunday must-haves (README, evidence, pitch) | Phase 1 tonight only if README/evidence are not at risk; Phase 2 only after them |

## 6. Decisions needed

1. Option C (native nodes + page)? Or B as a quick fallback?
2. Phase 1 only tonight, or Phase 1 + 2?
3. Dashboard location: `/poweredge-app/dashboard` behind the door (recommended), or public read-only for the stage screen?
