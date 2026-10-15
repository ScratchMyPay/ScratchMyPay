"""The wallet-owner must never send twice and never send without a matching approval."""

from __future__ import annotations

import threading

import pytest

from conftest import REVISION, plan_request
from poweredge_wallet.journal import Journal, JournalError, State
from poweredge_wallet.service import ServiceError, SimulatedCrash


def approved(service):
    plan = service.plan(plan_request())
    service.approve(plan["plan_id"], plan_digest=plan["plan_digest"],
                    flow_revision=REVISION, actor="Organizer")
    return plan


def test_happy_path_sends_once_delivers_and_settles(payer, recipient):
    receipts = []
    recipient_service = recipient.service()
    payer.deliver = lambda payload: receipts.append(payload) or recipient_service.import_consignment(payload)
    service = payer.service()

    plan = approved(service)
    result = service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")
    assert result["state"] == State.DELIVERED.value
    assert result["recipient_receipt"]["statuses"] == {7: "WAITING_CONFIRMATIONS"}
    assert len(payer.backend.sends) == 1 and payer.backend.sends[0][2] == 25_000_000
    assert recipient.backend.imported == receipts

    payer.backend.transfers[0] = payer.backend.transfers[0].__class__(
        **{**vars(payer.backend.transfers[0]), "status": "SETTLED"})
    final = service.status(plan["plan_id"], refresh=True)
    assert final["state"] == State.SETTLED.value
    assert [e["to_state"] for e in final["events"]] == [
        "PLANNED", "APPROVED", "SUBMITTING", "BROADCAST", "DELIVERED", "SETTLED"]


def test_failed_delivery_is_retried_and_settled_waits_for_the_recipient(payer, recipient):
    """Live run 2026-10-15: delivery hit an indexer race (502) and the payer then reported
    SETTLED although the crew wallet had not imported. Settled must wait for the import."""
    recipient_service = recipient.service()
    attempts = []

    def flaky(payload):
        attempts.append(payload)
        if len(attempts) == 1:
            raise RuntimeError("HTTP Error 502: indexer reported a height greater than the tip")
        return recipient_service.import_consignment(payload)

    payer.deliver = flaky
    service = payer.service()
    plan = approved(service)
    first = service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")
    assert first["state"] == State.BROADCAST.value and "delivery_error" in first

    # The payer's own transfer settles on-chain before the recipient imported.
    settled = payer.backend.transfers[0].__class__(**{**vars(payer.backend.transfers[0]), "status": "SETTLED"})
    payer.backend.transfers[0] = settled
    after = service.status(plan["plan_id"], refresh=True)
    assert len(attempts) == 2 and recipient.backend.imported == [attempts[0]]
    assert after["state"] == State.SETTLED.value
    assert [e["to_state"] for e in after["events"]][-3:] == ["BROADCAST", "DELIVERED", "SETTLED"]
    assert len(payer.backend.sends) == 1


def test_second_execute_never_sends_again(payer):
    service = payer.service()
    plan = approved(service)
    service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")
    again = service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000002")
    assert again["already_executed"] is True
    assert len(payer.backend.sends) == 1


def test_concurrent_executes_send_once(payer):
    service = payer.service()
    plan = approved(service)
    errors = []

    def run(i):
        try:
            service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key=f"key-race-{i:04d}")
        except ServiceError as exc:
            errors.append(exc.code)

    threads = [threading.Thread(target=run, args=(i,)) for i in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert len(payer.backend.sends) == 1, errors


def test_execute_requires_approval(payer):
    service = payer.service()
    plan = service.plan(plan_request())
    with pytest.raises(ServiceError) as err:
        service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")
    assert err.value.code == "not_approved"
    assert payer.backend.sends == []


@pytest.mark.parametrize("field,value,code", [
    ("plan_digest", "0" * 64, "digest_mismatch"),
    ("flow_revision", "b" * 40, "flow_changed"),
])
def test_approval_must_match_plan_and_flow(payer, field, value, code):
    service = payer.service()
    plan = service.plan(plan_request())
    kwargs = {"plan_digest": plan["plan_digest"], "flow_revision": REVISION, "actor": "Organizer"}
    kwargs[field] = value
    with pytest.raises(ServiceError) as err:
        service.approve(plan["plan_id"], **kwargs)
    assert err.value.code == code
    assert service.status(plan["plan_id"], refresh=False)["state"] == State.PLANNED.value


def test_flow_changed_after_approval_voids_it(payer):
    service = payer.service()
    plan = approved(service)
    with pytest.raises(ServiceError) as err:
        service.execute(plan["plan_id"], flow_revision="c" * 40, idempotency_key="key-000001")
    assert err.value.code == "flow_changed"
    assert service.status(plan["plan_id"], refresh=False)["state"] == State.FAILED.value
    assert payer.backend.sends == []


def test_crash_before_send_never_resends_and_reconciles_to_failed(payer):
    payer.crash_at = "before_send"
    service = payer.service()
    plan = approved(service)
    with pytest.raises(SimulatedCrash):
        service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")

    payer.crash_at = None
    restarted = payer.service()  # same journal file, fresh process state
    assert restarted.recovered == [plan["plan_id"]]
    again = restarted.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000002")
    assert again["state"] == State.UNKNOWN_SUBMISSION.value and again["already_executed"]
    assert payer.backend.sends == []
    assert restarted.reconcile(plan["plan_id"])["state"] == State.FAILED.value


def test_crash_after_send_keeps_the_broadcast_and_never_resends(payer):
    payer.crash_at = "after_send"
    service = payer.service()
    plan = approved(service)
    with pytest.raises(SimulatedCrash):
        service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")

    payer.crash_at = None
    restarted = payer.service()
    again = restarted.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000002")
    assert again["state"] == State.BROADCAST.value and again["txid"]
    assert len(payer.backend.sends) == 1


def test_crash_after_broadcast_before_journal_reconciles_without_resending(payer):
    """Process dies after the wallet broadcast but before BROADCAST is journaled."""
    payer.crash_at = "after_broadcast"
    service = payer.service()
    plan = approved(service)
    with pytest.raises(SimulatedCrash):
        service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")
    assert len(payer.backend.sends) == 1

    payer.crash_at = None
    restarted = payer.service()
    assert restarted.recovered == [plan["plan_id"]]
    again = restarted.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000002")
    assert again["state"] == State.UNKNOWN_SUBMISSION.value and again["already_executed"]
    reconciled = restarted.reconcile(plan["plan_id"])
    assert reconciled["state"] == State.BROADCAST.value
    assert reconciled["txid"] == payer.backend.transfers[0].txid
    assert len(payer.backend.sends) == 1  # never sent twice


def test_ambiguous_send_error_becomes_unknown_then_reconciles_to_broadcast(payer):
    payer.backend.fail_send_after_broadcast = True
    service = payer.service()
    plan = approved(service)
    with pytest.raises(ServiceError) as err:
        service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")
    assert err.value.code == "unknown_submission"
    reconciled = service.reconcile(plan["plan_id"])
    assert reconciled["state"] == State.BROADCAST.value
    assert reconciled["txid"] == payer.backend.transfers[0].txid
    assert len(payer.backend.sends) == 1


def test_balance_drift_is_refused_before_broadcast(payer):
    service = payer.service()
    plan = approved(service)
    payer.backend.balance -= 1  # someone else spent from this wallet
    with pytest.raises(ServiceError) as err:
        service.execute(plan["plan_id"], flow_revision=REVISION, idempotency_key="key-000001")
    assert err.value.code == "not_sent"
    assert service.status(plan["plan_id"], refresh=False)["state"] == State.FAILED.value
    assert payer.backend.sends == []


def test_expired_plan_cannot_be_approved(payer):
    service = payer.service()
    plan = service.plan(plan_request())
    service._cfg.clock = lambda: plan["expiration_timestamp"] + 1
    with pytest.raises(ServiceError) as err:
        service.approve(plan["plan_id"], plan_digest=plan["plan_digest"], flow_revision=REVISION, actor="x")
    assert err.value.code == "plan_expired"


def test_roles_are_enforced(payer, recipient):
    with pytest.raises(ServiceError) as err:
        recipient.service().plan(plan_request())
    assert err.value.code == "wrong_wallet"
    with pytest.raises(ServiceError) as err:
        payer.service().import_consignment(b"x")
    assert err.value.code == "wrong_wallet"


def test_plan_rejects_bad_amounts_and_unknown_assets(payer):
    service = payer.service()
    for overrides, code in [
        ({"amount_base_units": 0}, "invalid_amount"),
        ({"amount_base_units": True}, "invalid_amount"),
        ({"amount_base_units": 10**13, "amount_display": "10000000"}, "insufficient_balance"),
        ({"amount_base_units": 40_000_000}, "amount_mismatch"),  # shows "25", would send 40
        ({"amount_display": "25.0000001"}, "amount_mismatch"),
        ({"asset_ticker": "BTC"}, "unknown_asset"),
    ]:
        with pytest.raises(ServiceError) as err:
            service.plan(plan_request(**overrides))
        assert err.value.code == code, overrides


def test_first_time_recipient_any_asset_invoice_is_accepted(payer):
    payer.backend.invoice_asset = None  # recipient never held TUSDT: rgb-lib invoices "any asset"
    plan = payer.service().plan(plan_request())
    assert plan["asset_ticker"] == "TUSDT" and plan["recipient_id"] == "fake-recipient-1"


def test_invoice_for_another_asset_is_refused(payer):
    payer.backend.invoice_asset = "rgb:someone-elses-asset"
    with pytest.raises(ServiceError) as err:
        payer.service().plan(plan_request())
    assert err.value.code == "wallet_refused"


def test_journal_refuses_illegal_transitions(tmp_path):
    journal = Journal(tmp_path / "j.sqlite3")
    with pytest.raises(JournalError):
        journal.transition("missing", State.SETTLED, State.SUBMITTING)
