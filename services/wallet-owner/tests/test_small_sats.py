"""Small plain-sats payments (door passes): capped, rate-limited, journaled, payer only."""
import pytest

from poweredge_wallet.service import ServiceError

ADDR = "tb1q83x7zca8jdeqc6jpdh9d3ykzcs4pzj4jy2q9wc"


def test_pays_once_and_journals(payer):
    service = payer.service()
    out = service.pay_small_sats(address=ADDR, amount_sat=1000, fee_rate=2, purpose="door pass #1")
    assert out["txid"].startswith("btc") and payer.backend.btc_sends == [(ADDR, 1000, 2)]
    events = service._journal.events(f"sats:{out['txid']}")
    assert events and events[-1]["to_state"] == "NOTE" and "1000 sats" in events[-1]["detail"]


def test_caps_rate_limit_and_address(payer):
    service = payer.service()
    with pytest.raises(ServiceError) as big:
        service.pay_small_sats(address=ADDR, amount_sat=5001, fee_rate=2, purpose="x")
    assert big.value.code == "amount_too_large"
    with pytest.raises(ServiceError) as bad:
        service.pay_small_sats(address="bc1qmainnetaddressxxxxxxxxxxxxxxxxxx", amount_sat=10, fee_rate=2, purpose="x")
    assert bad.value.code == "invalid_address"
    service.pay_small_sats(address=ADDR, amount_sat=10, fee_rate=2, purpose="x")
    with pytest.raises(ServiceError) as fast:
        service.pay_small_sats(address=ADDR, amount_sat=10, fee_rate=2, purpose="x")
    assert fast.value.code == "rate_limited"
    assert len(payer.backend.btc_sends) == 1


def test_recipient_cannot_pay(recipient):
    with pytest.raises(ServiceError):
        recipient.service().pay_small_sats(address=ADDR, amount_sat=10, fee_rate=2, purpose="x")
