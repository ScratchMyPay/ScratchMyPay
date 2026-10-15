"""HTTP API for one wallet-owner process. Loopback only, bearer token, strict bodies."""

from __future__ import annotations

import hmac
import logging
from typing import Any

from fastapi import Depends, FastAPI, Header, Request
from fastapi.responses import JSONResponse
from pydantic import BaseModel, ConfigDict, Field

from .service import ServiceError, WalletService, decode_consignment

LOG = logging.getLogger("poweredge.api")
MAX_CONSIGNMENT_BYTES = 640 * 1024


class Strict(BaseModel):
    model_config = ConfigDict(extra="forbid")


class InvoiceRequest(Strict):
    # Omit for a first-time recipient: rgb-lib cannot name an asset the wallet never held.
    asset_id: str | None = Field(default=None, min_length=4, max_length=128)
    expires_seconds: int = Field(default=900, ge=60, le=86_400)


class ImportRequest(Strict):
    consignment_b64: str = Field(min_length=4, max_length=MAX_CONSIGNMENT_BYTES * 2)


class PlanRequest(Strict):
    invoice: str = Field(min_length=10, max_length=2048)
    asset_ticker: str = Field(min_length=1, max_length=8)
    amount_base_units: int = Field(gt=0)
    amount_display: str = Field(min_length=1, max_length=32)
    recipient_label: str = Field(min_length=1, max_length=64)
    memo: str = Field(default="", max_length=120)
    flow_revision: str = Field(min_length=8, max_length=128)
    template_digest: str = Field(min_length=64, max_length=64)


class ApproveRequest(Strict):
    plan_digest: str = Field(min_length=64, max_length=64)
    flow_revision: str = Field(min_length=8, max_length=128)
    actor: str = Field(min_length=1, max_length=64)


class ExecuteRequest(Strict):
    flow_revision: str = Field(min_length=8, max_length=128)
    idempotency_key: str = Field(min_length=8, max_length=128)


def create_app(service: WalletService, token: str) -> FastAPI:
    if len(token) < 32:
        raise ValueError("WALLET_API_TOKEN must be at least 32 characters")
    app = FastAPI(title=f"PowerEdge wallet-owner ({service.capabilities()['role']})",
                  docs_url=None, redoc_url=None, openapi_url=None)

    def auth(authorization: str = Header(default="")) -> None:
        supplied = authorization.removeprefix("Bearer ").strip()
        if not hmac.compare_digest(supplied.encode(), token.encode()):
            raise ServiceError("unauthorized", "missing or wrong wallet API token", 401)

    @app.exception_handler(ServiceError)
    async def service_error(_: Request, exc: ServiceError) -> JSONResponse:
        return JSONResponse(status_code=exc.status,
                            content={"error": {"code": exc.code, "message": str(exc)}})

    @app.get("/health")
    def health() -> dict[str, Any]:
        return {"ok": True, "role": service.capabilities()["role"]}

    guarded = [Depends(auth)]

    @app.get("/capabilities", dependencies=guarded)
    def capabilities() -> dict[str, Any]:
        return service.capabilities()

    @app.get("/assets", dependencies=guarded)
    def assets() -> dict[str, Any]:
        return {"assets": service.assets()}

    @app.post("/invoices", dependencies=guarded)
    def invoice(body: InvoiceRequest) -> dict[str, Any]:
        return service.create_invoice(asset_id=body.asset_id, expires_seconds=body.expires_seconds)

    @app.post("/consignments/import", dependencies=guarded)
    def import_consignment(body: ImportRequest) -> dict[str, Any]:
        return service.import_consignment(decode_consignment(body.consignment_b64, MAX_CONSIGNMENT_BYTES))

    @app.post("/plans", dependencies=guarded)
    def plan(body: PlanRequest) -> dict[str, Any]:
        return service.plan(body.model_dump())

    @app.post("/plans/{plan_id}/approve", dependencies=guarded)
    def approve(plan_id: str, body: ApproveRequest) -> dict[str, Any]:
        return service.approve(plan_id, plan_digest=body.plan_digest,
                               flow_revision=body.flow_revision, actor=body.actor)

    @app.post("/plans/{plan_id}/execute", dependencies=guarded)
    def execute(plan_id: str, body: ExecuteRequest) -> dict[str, Any]:
        return service.execute(plan_id, flow_revision=body.flow_revision,
                               idempotency_key=body.idempotency_key)

    @app.post("/plans/{plan_id}/reconcile", dependencies=guarded)
    def reconcile(plan_id: str) -> dict[str, Any]:
        return service.reconcile(plan_id)

    @app.post("/plans/{plan_id}/deliver", dependencies=guarded)
    def deliver(plan_id: str) -> dict[str, Any]:
        return service.deliver(plan_id)

    @app.get("/plans/{plan_id}", dependencies=guarded)
    def status(plan_id: str, refresh: bool = False) -> dict[str, Any]:
        return service.status(plan_id, refresh=refresh)

    @app.post("/refresh", dependencies=guarded)
    def refresh() -> dict[str, Any]:
        return service.refresh()

    return app
