"""The crew wallet settles incoming transfers by itself once they confirm."""
from poweredge_wallet.service import TransferRow


def test_no_refresh_without_pending_incoming(recipient):
    service = recipient.service()
    calls = []
    recipient.backend.refresh = lambda: calls.append(1) or {}
    recipient.backend.transfers.append(TransferRow(1, "SETTLED", "RECEIVE_WITNESS", "tx", "r", None))
    recipient.backend.transfers.append(TransferRow(2, "WAITING_CONFIRMATIONS", "SEND", "tx2", "r", None))
    assert service.auto_refresh_tick() is None
    assert calls == []


def test_refresh_while_incoming_is_pending(recipient):
    service = recipient.service()
    recipient.backend.refresh = lambda: {3: "SETTLED"}
    recipient.backend.transfers.append(TransferRow(3, "WAITING_CONFIRMATIONS", "RECEIVE_WITNESS", "tx", "r", None))
    assert service.has_pending_incoming() is True
    result = service.auto_refresh_tick()
    assert result is not None and result["updated_transfers"] == {3: "SETTLED"}
