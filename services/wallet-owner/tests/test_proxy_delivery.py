"""Payouts to invoices that name the RGB proxy: same exactly-once send, delivery via the proxy."""
from __future__ import annotations

import dataclasses
import time

import pytest

from conftest import REVISION, FakeBackend, Harness, backend_mod, plan_request
from poweredge_wallet.journal import State
from poweredge_wallet.service import ServiceError
from test_proxy import ALLOWED, INVOICE, OUTPUTS, RID


class ProxyBackend(FakeBackend):
    def parse_invoice(self, invoice: str):
        endpoints = tuple(e for e in invoice.split("endpoints=", 1)[1].split(",")) if "endpoints=" in invoice else ()
        return backend_mod.ParsedInvoice(invoice=invoice, recipient_id=RID, asset_id=None,
                                         expiration_timestamp=int(time.time()) + 3600,
                                         transport_endpoints=endpoints, unknown_query_params={})


class FakeProxy:
    def __init__(self):
        self.posts, self.fail_posts, self.ack = [], 0, None

    def post_consignment(self, endpoint, *, recipient_id, txid, vout, payload):
        if self.fail_posts:
            self.fail_posts -= 1
            raise RuntimeError("proxy unreachable")
        self.posts.append((endpoint, recipient_id, txid, vout, payload))
        return True

    def get_ack(self, endpoint, *, recipient_id):
        return self.ack


@pytest.fixture
def proxied(tmp_path):
    h = Harness(tmp_path, ProxyBackend(tmp_path), tmp_path / "payer.sqlite3", "payer")
    fake = FakeProxy()

    def make():
        service = h.service()
        service._cfg.allowed_proxies = ALLOWED
        service._cfg.proxy_client = fake
        service._cfg.tx_outputs = lambda txid: OUTPUTS
        return service
    return h, fake, make


def _approved(service, invoice=INVOICE):
    plan = service.plan(plan_request(invoice=invoice))
    service.approve(plan["plan_id"], plan_digest=plan["plan_digest"], flow_revision=REVISION, actor="Organizer")
    return plan


def _settle(h):
    h.backend.transfers[-1] = dataclasses.replace(h.backend.transfers[-1], status="SETTLED")
    h.backend.refresh = lambda: {}


def test_proxy_payout_posts_once_and_settles_only_after_ack(proxied):
    h, fake, make = proxied
    service = make()
    plan = _approved(service)
    out = service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")
    assert out["state"] == State.DELIVERED.value and out["delivered_via"] == ALLOWED[0]
    assert len(h.backend.sends) == 1 and len(fake.posts) == 1
    endpoint, rid, txid, vout, payload = fake.posts[0]
    assert (endpoint, rid, vout) == (ALLOWED[0], RID, 1) and payload.startswith(b"consignment-bytes-")
    _settle(h)
    assert service.status(plan["plan_id"], refresh=True)["state"] == State.DELIVERED.value  # no ACK yet
    fake.ack = True
    assert service.status(plan["plan_id"], refresh=True)["state"] == State.SETTLED.value
    assert len(h.backend.sends) == 1


def test_proxy_outage_retries_delivery_never_resends(proxied):
    h, fake, make = proxied
    fake.fail_posts = 1
    service = make()
    plan = _approved(service)
    out = service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")
    assert out["state"] == State.BROADCAST.value and "proxy unreachable" in out["delivery_error"]
    again = service.status(plan["plan_id"], refresh=True)  # retries the upload only
    assert again["state"] == State.DELIVERED.value
    assert len(h.backend.sends) == 1 and len(fake.posts) == 1


def test_unknown_proxy_is_refused_before_anything_is_planned(proxied):
    h, _, make = proxied
    service = make()
    with pytest.raises(ServiceError) as exc:
        service.plan(plan_request(invoice=INVOICE.replace("proxy.kaleidoswap.com", "evil.example")))
    assert "not allowed" in str(exc.value)
    assert h.backend.sends == []


def test_proxy_invoices_for_funding_and_crew(proxied, recipient):
    _, _, make = proxied
    payer_service = make()
    with pytest.raises(ServiceError):
        payer_service.create_invoice(asset_id=None, expires_seconds=900)          # payer: no direct invoices
    funding = payer_service.create_invoice(asset_id=None, expires_seconds=900, transport="proxy")
    assert funding["transport"] == "proxy" and funding["proxy"] == ALLOWED[0] and "endpoints=" in funding["invoice"]
    crew = recipient.service()
    with pytest.raises(ServiceError) as exc:                                       # no proxy configured
        crew.create_invoice(asset_id=None, expires_seconds=900, transport="proxy")
    assert exc.value.code == "proxy_disabled"
