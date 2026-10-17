"""Dashboard statistics: per-hour payouts, volume, time to settle, recent payouts. Read-only."""
from __future__ import annotations

from fastapi.testclient import TestClient

from conftest import REVISION, plan_request
from poweredge_wallet.api import create_app
from poweredge_wallet.journal import State, _percentile

TOKEN = "t" * 64


def _settle_one(payer, recipient, service, **overrides):
    recipient_service = recipient.service()
    payer.deliver = lambda payload: recipient_service.import_consignment(payload)
    plan = service.plan(plan_request(**overrides))
    service.approve(plan["plan_id"], plan_digest=plan["plan_digest"], flow_revision=REVISION, actor="Organizer")
    service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key=f"key-{plan['plan_id'][:8]}")
    i = len(payer.backend.transfers) - 1
    payer.backend.transfers[i] = payer.backend.transfers[i].__class__(**{**vars(payer.backend.transfers[i]), "status": "SETTLED"})
    assert service.status(plan["plan_id"], refresh=True)["state"] == State.SETTLED.value
    return plan


def test_stats_count_settled_volume_and_time_to_settle(payer, recipient):
    service = payer.service()
    _settle_one(payer, recipient, service)
    service.plan(plan_request(amount_display="3", amount_base_units=3_000_000))   # prepared, never approved
    stats = service.stats(hours=24)
    assert stats["hours"] == 24 and len(stats["buckets"]) == 24
    assert stats["totals"]["settled"] == 1 and stats["totals"]["prepared"] == 2
    assert stats["totals"]["volume"] == {"TUSDT": 25.0}   # the wallet's own ticker
    assert sum(b["settled"] for b in stats["buckets"]) == 1
    assert stats["settleSeconds"]["n"] == 1 and stats["settleSeconds"]["p50"] is not None


def test_recent_plans_newest_first_without_invoice(payer, recipient):
    service = payer.service()
    first = service.plan(plan_request(memo="first"))
    second = service.plan(plan_request(memo="second"))
    rows = service.recent_plans(limit=10)
    assert [r["plan_id"] for r in rows[:2]] in ([second["plan_id"], first["plan_id"]], [first["plan_id"], second["plan_id"]])
    assert all("invoice" not in r and "consignment_path" not in r for r in rows)


def test_api_routes_are_guarded_and_bounded(payer):
    client = TestClient(create_app(payer.service(), TOKEN))
    assert client.get("/stats").status_code == 401
    auth = {"Authorization": f"Bearer {TOKEN}"}
    assert client.get("/stats?hours=9999", headers=auth).json()["hours"] == 168
    assert client.get("/plans?limit=0", headers=auth).status_code == 200


def test_percentile_nearest_rank():
    assert _percentile([], 50) is None
    assert _percentile([10, 20, 30, 40], 50) == 20
    assert _percentile([10, 20, 30, 40], 90) == 40
