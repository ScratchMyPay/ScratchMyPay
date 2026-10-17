"""RGB proxy helpers, checked against the real KaleidoSwap faucet transfer of 2026-10-17."""
import base64

import pytest

from poweredge_wallet import proxy

# Real Mutinynet receive (KaleidoSwap faucet -> our interop wallet), txid 9753e744...a290e5ce
RID = "sbc:wvout:BZ~JLPsb-yIvzUED-XHTsORu-bq24Dwc-hBkO4S2-JQ3od_l-h58HBgw"
INVOICE = f"rgb:~/~/~/{RID}?expiry=1792316840&endpoints=rpcs://proxy.kaleidoswap.com/json-rpc"
OUTPUTS = [
    (0, "6a20a89ce8ee094ef127b6bfe1e33ad80c758629b0a0f6ac31b5c30ba8d2def6d05d"),
    (1000, "51209fc92cfb1bc88bf35040d71d3b0e46e6eadb80f07210643b84b6250de877e961"),
    (198866, "512034af168231b70dfb136a114772dbf455233eeaf47c8da3b2d4a529c7af33557f"),
]
ALLOWED = ("rpcs://proxy.kaleidoswap.com/json-rpc",)


def test_witness_script_and_vout_match_the_real_transfer():
    script = proxy.witness_script(RID)
    assert script.hex() == OUTPUTS[1][1]
    assert proxy.find_vout(OUTPUTS, script) == 1  # the proxy told the receiver vout=1


def test_invoice_endpoints_and_allowlist():
    assert proxy.invoice_endpoints(INVOICE) == ["rpcs://proxy.kaleidoswap.com/json-rpc"]
    assert proxy.proxy_for(INVOICE, ALLOWED) == ALLOWED[0]
    assert proxy.proxy_for(f"rgb:~/~/~/{RID}?expiry=1", ALLOWED) is None
    with pytest.raises(proxy.ProxyError):
        proxy.proxy_for(INVOICE.replace("proxy.kaleidoswap.com", "evil.example"), ALLOWED)
    with pytest.raises(proxy.ProxyError):
        proxy.proxy_for(INVOICE + ",rpcs://other/json-rpc", ALLOWED)
    assert proxy.http_url(ALLOWED[0]) == "https://proxy.kaleidoswap.com/json-rpc"
    with pytest.raises(proxy.ProxyError):
        proxy.http_url("rpc://proxy.example/json-rpc")  # plain HTTP only for localhost


def test_unsupported_recipients_refused():
    with pytest.raises(proxy.ProxyError):
        proxy.witness_script("sbc:utxob:abc")
    with pytest.raises(proxy.ProxyError):
        proxy.find_vout(OUTPUTS, bytes.fromhex("5120" + "00" * 32))


class _Resp:
    def __init__(self, body):
        self._body = body
    def raise_for_status(self):
        pass
    def json(self):
        return self._body


def test_post_is_idempotent_when_our_file_is_already_there():
    payload = b"consignment-bytes"
    calls = []

    def post(url, **kw):
        calls.append(kw)
        if "data" in kw:  # consignment.post
            return _Resp({"error": {"message": "Cannot change uploaded file"}})
        return _Resp({"result": {"txid": "ab" * 32, "vout": 1, "consignment": base64.b64encode(payload).decode()}})

    client = proxy.ProxyClient(post=post)
    assert client.post_consignment(ALLOWED[0], recipient_id=RID, txid="ab" * 32, vout=1, payload=payload)
    assert calls[0]["data"]["method"] == "consignment.post" and calls[0]["data"]["params[vout]"] == "1"
    # a different file under the same id is a real error, not success
    with pytest.raises(proxy.ProxyError):
        client.post_consignment(ALLOWED[0], recipient_id=RID, txid="ab" * 32, vout=1, payload=b"other")
