"""HTTP contract: auth, strict bodies, stable error codes, full payout over HTTP."""

from __future__ import annotations

from fastapi.testclient import TestClient

from conftest import REVISION, plan_request
from poweredge_wallet.api import create_app

TOKEN = "t" * 64
AUTH = {"Authorization": f"Bearer {TOKEN}"}


def test_requires_token_and_rejects_unknown_fields(payer):
    client = TestClient(create_app(payer.service(), TOKEN))
    assert client.get("/health").status_code == 200
    assert client.get("/assets").status_code == 401
    assert client.get("/assets", headers={"Authorization": "Bearer wrong"}).status_code == 401
    bad = client.post("/plans", headers=AUTH, json={**plan_request(), "node_type": "exec"})
    assert bad.status_code == 422


def test_payout_over_http(payer):
    client = TestClient(create_app(payer.service(), TOKEN))
    assets = client.get("/assets", headers=AUTH).json()["assets"]
    assert assets[0]["ticker"] == "TUSDT" and assets[0]["precision"] == 6

    plan = client.post("/plans", headers=AUTH, json=plan_request()).json()
    early = client.post(f"/plans/{plan['plan_id']}/execute", headers=AUTH,
                        json={"flow_revision": REVISION, "idempotency_key": "key-000001"})
    assert early.status_code == 409 and early.json()["error"]["code"] == "not_approved"

    ok = client.post(f"/plans/{plan['plan_id']}/approve", headers=AUTH,
                     json={"plan_digest": plan["plan_digest"], "flow_revision": REVISION, "actor": "Organizer"})
    assert ok.json()["state"] == "APPROVED"
    sent = client.post(f"/plans/{plan['plan_id']}/execute", headers=AUTH,
                       json={"flow_revision": REVISION, "idempotency_key": "key-000002"}).json()
    assert sent["state"] == "BROADCAST" and "consignment_path" not in sent
    status = client.get(f"/plans/{plan['plan_id']}", headers=AUTH).json()
    assert status["txid"] == sent["txid"] and len(payer.backend.sends) == 1


def test_wrong_role_and_unknown_plan_codes(recipient):
    client = TestClient(create_app(recipient.service(), TOKEN))
    assert client.post("/plans", headers=AUTH, json=plan_request()).json()["error"]["code"] == "wrong_wallet"
    missing = client.get("/plans/00000000-0000-0000-0000-000000000000", headers=AUTH)
    assert missing.status_code == 404 and missing.json()["error"]["code"] == "unknown_plan"
