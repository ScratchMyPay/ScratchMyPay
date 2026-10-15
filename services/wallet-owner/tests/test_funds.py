"""Read-only funds report: UTXO inventory and payout estimate."""
from fastapi.testclient import TestClient

from poweredge_wallet.api import create_app

TOKEN = "t" * 40


def test_funds_summary(payer):
    service = payer.service()
    f = service.funds(fee_rate=2)
    assert f["utxos"]["total"] == 4
    assert f["utxos"]["colorable"] == 3
    assert f["utxos"]["holding_assets"] == 1
    assert f["utxos"]["free_colorable"] == 2
    assert f["utxos"]["free_colorable_sats"] == [10000, 6395]
    assert f["estimate"] == {"fee_rate_sat_vb": 2, "per_payout_sat": 1394, "payouts_from_colored_sats": 17}


def test_funds_http_contract(payer):
    client = TestClient(create_app(payer.service(), TOKEN))
    assert client.get("/funds").status_code == 401
    ok = client.get("/funds?fee_rate=1", headers={"Authorization": f"Bearer {TOKEN}"})
    assert ok.status_code == 200 and ok.json()["estimate"]["per_payout_sat"] == 1197
    bad = client.get("/funds?fee_rate=0", headers={"Authorization": f"Bearer {TOKEN}"})
    assert bad.status_code == 400 and bad.json()["error"]["code"] == "invalid_request"
