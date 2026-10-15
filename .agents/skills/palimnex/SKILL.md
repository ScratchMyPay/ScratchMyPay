---
name: palimnex
description: Consult and maintain this repository's Palimnex source cache and durable historical-memory ledger for non-trivial repository work.
---

# Palimnex workflow (RGB2link PowerEdge)

Use the installed `palimnex` 2.9.0 command from the repository root. The Palimnex source checkout is `/home/kestl/github/palimnex`.

1. Run `palimnex status`.
2. If the cache is missing or stale, run `palimnex index --incremental`.
3. Run `palimnex validate --deep`.
4. Search with exact implementation vocabulary and open the returned source.
5. After indexed-file changes, reindex and run `palimnex validate --deep`.
6. Run `palimnex ledger-status` for handoffs.
7. Finish install or health checks with `palimnex doctor`.

n2link reference source (separate Palimnex project, do not merge caches):

```bash
PALIMNEX_ROOT=/media/kestl/andor/github/n2link palimnex status
PALIMNEX_ROOT=/media/kestl/andor/github/n2link palimnex search "flow node registration" --limit 5
PALIMNEX_ROOT=/media/kestl/andor/github/n2link palimnex symbols "flow_node" --limit 8
```

Pinned checkout metadata: `docs/reference-sources/n2link.json`. Open hits under `/media/kestl/andor/github/n2link` at that SHA.

Redis is optional and disposable; SQLite is durable historical context. Never treat either as current authorization. If Redis is unavailable, continue from files and disclose `cache_consulted: false`. Never flush shared Redis. Never commit `.palimnex/`, `*.pmem`, or `*.key`.
