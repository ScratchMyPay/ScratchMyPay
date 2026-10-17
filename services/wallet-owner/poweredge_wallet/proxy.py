"""RGB proxy delivery (RGB HTTP JSON-RPC, e.g. KaleidoSwap's proxy on Mutinynet).

Bill sends exactly as before (plan -> confirm -> execute-once, consignment written to disk).
Only *delivery* changes when the crew's invoice names a proxy: instead of handing the
consignment straight to our own crew service, we upload it to the proxy, where any RGB
wallet (ours, KaleidoSwap's, Rate) fetches it, validates it and posts an ACK.

Pure helpers here are unit-tested; HTTP is done with httpx and injectable.
"""
from __future__ import annotations

import base64
import os
from dataclasses import dataclass
from typing import Any, Callable
from urllib.parse import parse_qs, urlsplit

DEFAULT_PROXIES = "rpcs://proxy.kaleidoswap.com/json-rpc"


class ProxyError(RuntimeError):
    pass


def allowed_proxies() -> tuple[str, ...]:
    """Proxies we may upload a consignment to (`POWEREDGE_RGB_PROXY`, comma-separated)."""
    raw = os.environ.get("POWEREDGE_RGB_PROXY", DEFAULT_PROXIES)
    return tuple(p.strip() for p in raw.split(",") if p.strip())


def invoice_endpoints(invoice: str | None) -> list[str]:
    """Transport endpoints named in an RGB invoice string (`?endpoints=a,b`)."""
    if not invoice or "?" not in invoice:
        return []
    query = parse_qs(invoice.split("?", 1)[1], keep_blank_values=False)
    return [e for v in query.get("endpoints", []) for e in v.split(",") if e]


def proxy_for(invoice: str | None, allowed: tuple[str, ...]) -> str | None:
    """The single allowed proxy the invoice asks for, None for direct delivery.

    Raises if the invoice names a proxy we do not allow, or several (we post to exactly one).
    """
    endpoints = invoice_endpoints(invoice)
    if not endpoints:
        return None
    if len(endpoints) != 1:
        raise ProxyError("the invoice names several transport endpoints; exactly one proxy is supported")
    if endpoints[0] not in allowed:
        raise ProxyError(f"the invoice asks for an RGB proxy that is not allowed here: {endpoints[0]}")
    return endpoints[0]


def http_url(endpoint: str) -> str:
    """rpcs:// -> https://, rpc:// -> http:// (plain HTTP only for local test proxies)."""
    parts = urlsplit(endpoint)
    if parts.scheme == "rpcs":
        return "https://" + endpoint.split("://", 1)[1]
    if parts.scheme == "rpc" and parts.hostname in ("127.0.0.1", "localhost"):
        return "http://" + endpoint.split("://", 1)[1]
    raise ProxyError(f"unsupported RGB proxy endpoint: {endpoint}")


def _baid64_decode(text: str) -> bytes:
    """RGB's base64 variant: standard alphabet with '~' for 63 and '_' for 62, '-' as separators."""
    t = text.replace("-", "").translate(str.maketrans("~_", "/+"))
    return base64.b64decode(t + "=" * (-len(t) % 4))


def witness_script(recipient_id: str) -> bytes:
    """Output script a witness recipient id ('sbc:wvout:...') pays to. Taproot only.

    Layout (verified against a real Mutinynet receive): 0x05 (P2TR) | 32-byte x-only key | 4-byte checksum.
    """
    body = recipient_id.split("wvout:", 1)[1] if "wvout:" in recipient_id else None
    if not body:
        raise ProxyError("not a witness recipient (wvout)")
    raw = _baid64_decode(body)
    if len(raw) != 37 or raw[0] != 0x05:
        raise ProxyError("unsupported witness recipient (only Taproot outputs are supported)")
    return bytes([0x51, 0x20]) + raw[1:33]


def find_vout(outputs: list[tuple[int, str]], script: bytes) -> int:
    """Index of the one output paying `script`; outputs are (value_sat, scriptpubkey_hex)."""
    matches = [i for i, (_, spk) in enumerate(outputs) if bytes.fromhex(spk) == script]
    if len(matches) != 1:
        raise ProxyError(f"expected exactly one output paying the recipient, found {len(matches)}")
    return matches[0]


@dataclass
class ProxyClient:
    """Minimal RGB HTTP JSON-RPC client."""
    post: Callable[..., Any] | None = None  # injectable for tests: (url, **kwargs) -> response-like
    timeout: float = 30.0

    def _post(self, url: str, **kwargs: Any) -> dict[str, Any]:
        if self.post is not None:
            resp = self.post(url, **kwargs)
        else:
            import httpx
            resp = httpx.post(url, timeout=self.timeout, **kwargs)
        resp.raise_for_status()
        body = resp.json()
        if body.get("error"):
            raise ProxyError(f"proxy error: {body['error'].get('message', body['error'])}")
        return body

    def post_consignment(self, endpoint: str, *, recipient_id: str, txid: str, vout: int, payload: bytes) -> bool:
        url = http_url(endpoint)
        data = {"jsonrpc": "2.0", "id": "1", "method": "consignment.post",
                "params[recipient_id]": recipient_id, "params[txid]": txid, "params[vout]": str(vout)}
        try:
            return bool(self._post(url, data=data, files={"file": ("consignment.rgb", payload, "application/octet-stream")}).get("result"))
        except ProxyError:
            # The proxy never replaces an uploaded file. If ours is already there, delivery is done.
            existing = self.get_consignment(endpoint, recipient_id=recipient_id)
            if existing and existing.get("txid") == txid and base64.b64decode(existing.get("consignment", "")) == payload:
                return True
            raise

    def get_consignment(self, endpoint: str, *, recipient_id: str) -> dict[str, Any] | None:
        body = self._post(http_url(endpoint), json={"jsonrpc": "2.0", "id": "2", "method": "consignment.get",
                                                    "params": {"recipient_id": recipient_id}})
        return body.get("result")

    def get_ack(self, endpoint: str, *, recipient_id: str) -> bool | None:
        """True = the payee validated it, False = rejected (NACK), None = not checked yet."""
        body = self._post(http_url(endpoint), json={"jsonrpc": "2.0", "id": "3", "method": "ack.get",
                                                    "params": {"recipient_id": recipient_id}})
        return body.get("result")


def esplora_outputs(base_url: str, txid: str, *, get: Callable[..., Any] | None = None) -> list[tuple[int, str]]:
    """(value_sat, scriptpubkey_hex) for each output of `txid`, from an Esplora indexer."""
    if get is None:
        import httpx
        resp = httpx.get(f"{base_url.rstrip('/')}/tx/{txid}", timeout=20.0)
    else:
        resp = get(f"{base_url.rstrip('/')}/tx/{txid}")
    resp.raise_for_status()
    return [(int(o["value"]), o["scriptpubkey"]) for o in resp.json()["vout"]]
