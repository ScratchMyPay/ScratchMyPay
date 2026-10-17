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


def test_refresh_sees_receives_not_yet_tied_to_an_asset(recipient):
    """A first-time any-asset invoice belongs to no asset until fetched (crew's first USDT)."""
    service = recipient.service()
    seen = []
    original = service._ops.transfers

    def transfers(asset_id):
        seen.append(asset_id)
        if asset_id is None:
            return [TransferRow(4, "WAITING_COUNTERPARTY", "RECEIVE_WITNESS", None, "r", None)]
        return original(asset_id)

    service._ops.transfers = transfers
    recipient.backend.refresh = lambda: {4: "WAITING_CONFIRMATIONS"}
    assert service.has_pending_incoming() is True and None in seen
    assert service.auto_refresh_tick()["updated_transfers"] == {4: "WAITING_CONFIRMATIONS"}


def test_refresh_while_incoming_is_pending(recipient):
    service = recipient.service()
    recipient.backend.refresh = lambda: {3: "SETTLED"}
    recipient.backend.transfers.append(TransferRow(3, "WAITING_CONFIRMATIONS", "RECEIVE_WITNESS", "tx", "r", None))
    assert service.has_pending_incoming() is True
    result = service.auto_refresh_tick()
    assert result is not None and result["updated_transfers"] == {3: "SETTLED"}
