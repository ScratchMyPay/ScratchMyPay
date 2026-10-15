//! RGB2link PowerEdge nodes (`nodes_rgb`). Thin, unprivileged clients of the
//! wallet-owner service: they never see keys and never decide to spend.
//!
//! Message contract: every node writes `msg.payload = {ok, step, ...}`; on failure
//! `{ok: false, step, error: {code, message}}` plus `msg.statusCode`, and the message goes
//! to the node's error output (the last port) so an `http response` can answer it.
//! The payout plan travels in `msg.poweredge = {planId, planDigest, flowRevision}`.

mod client;
mod qr;

use std::sync::Arc;
use std::time::Duration;

use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::N2linkError;
use crate::runtime::flow::Flow;
use crate::runtime::model::json::RedFlowNodeConfig;
use crate::runtime::model::{Msg, MsgHandle, Variant};
use crate::runtime::nodes::*;
use n2link_macro::*;

use client::{CallError, WalletHandle, wallet_from_flow};

const APPROVAL_POLL: Duration = Duration::from_secs(2);
const SETTLE_POLL: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------------------
// shared helpers

#[derive(Debug, Deserialize)]
struct WalletOnly {
    #[serde(default)]
    wallet: String,
}

/// The editor saves text inputs as strings; accept a non-negative integer either way.
fn u64_or_str<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Value::deserialize(deserializer)? {
        Value::Number(n) => n.as_u64().ok_or_else(|| serde::de::Error::custom("expected a non-negative integer")),
        Value::String(s) if s.trim().is_empty() => Ok(0),
        Value::String(s) => s.trim().parse().map_err(|_| serde::de::Error::custom("expected a non-negative integer")),
        Value::Null => Ok(0),
        _ => Err(serde::de::Error::custom("expected a non-negative integer")),
    }
}

fn require_wallet(reference: &str, node: &str) -> crate::Result<String> {
    if reference.trim().is_empty() {
        return Err(N2linkError::invalid_operation(&format!("{node} needs an rgb-wallet-ref")));
    }
    Ok(reference.trim().to_owned())
}

fn payload_json(msg: &Msg) -> Value {
    msg.get("payload").and_then(|v| serde_json::to_value(v).ok()).unwrap_or(Value::Null)
}

fn poweredge_json(msg: &Msg) -> Value {
    msg.get("poweredge").and_then(|v| serde_json::to_value(v).ok()).unwrap_or(Value::Null)
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_owned()
}

async fn set_payload(msg: &MsgHandle, payload: Value, status: Option<u16>) {
    let mut guard = msg.write().await;
    guard.set("payload".to_owned(), Variant::from(payload));
    if let Some(code) = status {
        guard.set("statusCode".to_owned(), Variant::from(json!(code)));
    }
}

fn status(fill: StatusFill, shape: StatusShape, text: impl Into<String>) -> StatusObject {
    StatusObject { fill: Some(fill), shape: Some(shape), text: Some(text.into()) }
}

/// Write the error payload and route the message to `port`.
async fn fail<N: FlowNodeBehavior + ?Sized>(
    node: &N,
    msg: MsgHandle,
    port: usize,
    step: &str,
    err: CallError,
    cancel: CancellationToken,
) -> crate::Result<()> {
    log::warn!("[{step}] wallet call failed: {} {}", err.code, err.message);
    set_payload(&msg, err.to_json(step), Some(err.status)).await;
    node.report_status(status(StatusFill::Red, StatusShape::Ring, err.code.clone()), cancel.clone()).await;
    node.fan_out_one(Envelope { port, msg }, cancel).await
}

async fn flow_revision(flow: &Flow) -> crate::Result<String> {
    let engine = flow.engine().ok_or_else(|| N2linkError::invalid_operation("RGB node has no engine"))?;
    Ok(engine.flows_rev().await)
}

fn flow_of<N: FlowNodeBehavior + ?Sized>(node: &N) -> crate::Result<Flow> {
    node.flow().ok_or_else(|| N2linkError::invalid_operation("RGB node has no flow"))
}

fn plan_id_of(msg: &Msg) -> Option<String> {
    let id = text(&poweredge_json(msg), "planId");
    (!id.is_empty()).then_some(id)
}

fn missing_plan() -> CallError {
    CallError { status: 400, code: "missing_plan".into(), message: "message carries no payout plan".into() }
}

// ---------------------------------------------------------------------------------------
// rgb-invoice: recipient asks for a payment (any-asset invoice, out-of-band delivery)

#[derive(Debug, Deserialize)]
struct InvoiceConfig {
    #[serde(default)]
    wallet: String,
    #[serde(default, rename = "assetTicker")]
    asset_ticker: String,
}

crate::node_hints!("rgb-invoice", refs = ["wallet" => "rgb-wallet-ref"], caps = ["wallet", "network"], input = "object", outs = ["invoice" => "object"]);

#[flow_node("rgb-invoice", red_name = "rgb-invoice", inputs = 1, outputs = 1)]
struct RgbInvoiceNode {
    base: BaseFlowNodeState,
    wallet: String,
    asset_ticker: String,
}

impl RgbInvoiceNode {
    fn build(
        _flow: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = InvoiceConfig::deserialize(&config.rest)?;
        Ok(Box::new(RgbInvoiceNode {
            base,
            wallet: require_wallet(&raw.wallet, "rgb-invoice")?,
            asset_ticker: raw.asset_ticker,
        }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let wallet = wallet_from_flow(&flow_of(self)?, &self.wallet, "recipient")?;
        let input = payload_json(&*msg.read().await);
        let expires = input.get("expiresSeconds").and_then(Value::as_u64).unwrap_or(3600).clamp(300, 86_400);
        match wallet.call(Method::POST, "/invoices", Some(&json!({ "expires_seconds": expires }))).await {
            Ok(invoice) => {
                let requested = json!({
                    "amount": text(&input, "amount"),
                    "label": text(&input, "label"),
                    "assetTicker": self.asset_ticker,
                });
                set_payload(
                    &msg,
                    json!({ "ok": true, "step": "invoice", "invoice": invoice, "requested": requested }),
                    None,
                )
                .await;
                self.report_status(status(StatusFill::Green, StatusShape::Dot, "invoice ready"), cancel.clone()).await;
                self.fan_out_one(Envelope { port: 0, msg }, cancel).await
            }
            Err(err) => fail(self, msg, 0, "invoice", err, cancel).await,
        }
    }
}

#[async_trait]
impl FlowNodeBehavior for RgbInvoiceNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move { node.handle(msg, cancel).await })
                .await;
        }
    }
}

// ---------------------------------------------------------------------------------------
// rgb-plan: payer prepares an immutable plan bound to the deployed flow revision

#[derive(Debug, Deserialize)]
struct PlanConfig {
    #[serde(default)]
    wallet: String,
    #[serde(default, rename = "recipientLabel")]
    recipient_label: String,
    #[serde(default, rename = "assetTicker")]
    asset_ticker: String,
    #[serde(default, rename = "amountDisplay")]
    amount_display: String,
    #[serde(default, rename = "amountBaseUnits", deserialize_with = "u64_or_str")]
    amount_base_units: u64,
    #[serde(default)]
    memo: String,
    #[serde(default, rename = "templateDigest")]
    template_digest: String,
}

crate::node_hints!("rgb-plan", refs = ["wallet" => "rgb-wallet-ref"], caps = ["wallet", "network"], input = "object", outs = ["plan" => "object", "error" => "object"]);

#[flow_node("rgb-plan", red_name = "rgb-plan", inputs = 1, outputs = 2)]
struct RgbPlanNode {
    base: BaseFlowNodeState,
    wallet: String,
    config: PlanConfig,
}

impl RgbPlanNode {
    fn build(
        _flow: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = PlanConfig::deserialize(&config.rest)?;
        if raw.amount_base_units == 0 || raw.amount_display.trim().is_empty() {
            return Err(N2linkError::invalid_operation("rgb-plan needs a compiled amount"));
        }
        if raw.template_digest.len() != 64 || raw.recipient_label.trim().is_empty() {
            return Err(N2linkError::invalid_operation("rgb-plan must come from a pinned template"));
        }
        Ok(Box::new(RgbPlanNode { base, wallet: require_wallet(&raw.wallet, "rgb-plan")?, config: raw }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let flow = flow_of(self)?;
        let wallet = wallet_from_flow(&flow, &self.wallet, "payer")?;
        let invoice = text(&payload_json(&*msg.read().await), "invoice");
        if invoice.is_empty() {
            let err = CallError {
                status: 400,
                code: "missing_invoice".into(),
                message: "paste the recipient's invoice".into(),
            };
            return fail(self, msg, 1, "plan", err, cancel).await;
        }
        let revision = flow_revision(&flow).await?;
        let body = json!({
            "invoice": invoice,
            "asset_ticker": self.config.asset_ticker,
            "amount_base_units": self.config.amount_base_units,
            "amount_display": self.config.amount_display,
            "recipient_label": self.config.recipient_label,
            "memo": self.config.memo,
            "flow_revision": revision,
            "template_digest": self.config.template_digest,
        });
        match wallet.call(Method::POST, "/plans", Some(&body)).await {
            Ok(plan) => {
                let poweredge = json!({
                    "planId": text(&plan, "plan_id"),
                    "planDigest": text(&plan, "plan_digest"),
                    "flowRevision": revision,
                });
                {
                    let mut guard = msg.write().await;
                    guard.set("poweredge".to_owned(), Variant::from(poweredge));
                    guard.set(
                        "payload".to_owned(),
                        Variant::from(json!({ "ok": true, "step": "awaiting_approval", "payout": plan })),
                    );
                }
                self.report_status(status(StatusFill::Yellow, StatusShape::Dot, "awaiting approval"), cancel.clone())
                    .await;
                self.fan_out_one(Envelope { port: 0, msg }, cancel).await
            }
            Err(err) => fail(self, msg, 1, "plan", err, cancel).await,
        }
    }
}

#[async_trait]
impl FlowNodeBehavior for RgbPlanNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move { node.handle(msg, cancel).await })
                .await;
        }
    }
}

// ---------------------------------------------------------------------------------------
// approval-gate: answers "awaiting approval" at once (port 1), then holds the message
// until the wallet journal shows a human approval for exactly this plan (port 0).

#[derive(Debug, Deserialize)]
struct GateConfig {
    #[serde(default)]
    wallet: String,
    #[serde(default = "default_gate_timeout", rename = "timeoutSeconds", deserialize_with = "u64_or_str")]
    timeout_seconds: u64,
}

fn default_gate_timeout() -> u64 {
    900
}

crate::node_hints!("approval-gate", refs = ["wallet" => "rgb-wallet-ref"], caps = ["wallet", "network"], input = "object", outs = ["approved" => "object", "pending" => "object"]);

#[flow_node("approval-gate", red_name = "approval-gate", inputs = 1, outputs = 2)]
struct ApprovalGateNode {
    base: BaseFlowNodeState,
    wallet: String,
    timeout: Duration,
}

impl ApprovalGateNode {
    fn build(
        _flow: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = GateConfig::deserialize(&config.rest)?;
        if !(60..=3600).contains(&raw.timeout_seconds) {
            return Err(N2linkError::invalid_operation("approval-gate timeoutSeconds must be 60..3600"));
        }
        Ok(Box::new(ApprovalGateNode {
            base,
            wallet: require_wallet(&raw.wallet, "approval-gate")?,
            timeout: Duration::from_secs(raw.timeout_seconds),
        }))
    }

    async fn wait(
        &self,
        msg: MsgHandle,
        wallet: WalletHandle,
        plan_id: String,
        cancel: CancellationToken,
    ) -> crate::Result<()> {
        let deadline = tokio::time::Instant::now() + self.timeout;
        loop {
            if tokio::time::Instant::now() >= deadline {
                self.report_status(status(StatusFill::Red, StatusShape::Ring, "approval timed out"), cancel).await;
                return Ok(());
            }
            tokio::select! {
                _ = cancel.cancelled() => return Err(N2linkError::TaskCancelled),
                _ = tokio::time::sleep(APPROVAL_POLL) => {}
            }
            let view = match wallet.call(Method::GET, &format!("/plans/{plan_id}"), None).await {
                Ok(view) => view,
                Err(err) if err.code == "wallet_unreachable" => continue,
                Err(err) => {
                    self.report_status(status(StatusFill::Red, StatusShape::Ring, err.code), cancel).await;
                    return Ok(());
                }
            };
            match text(&view, "state").as_str() {
                "PLANNED" => continue,
                "APPROVED" => {
                    let who = text(&view, "approved_by");
                    set_payload(&msg, json!({ "ok": true, "step": "approved", "payout": view }), None).await;
                    self.report_status(
                        status(StatusFill::Green, StatusShape::Dot, format!("approved by {who}")),
                        cancel.clone(),
                    )
                    .await;
                    return self.fan_out_one(Envelope { port: 0, msg }, cancel).await;
                }
                other => {
                    self.report_status(status(StatusFill::Red, StatusShape::Ring, other.to_lowercase()), cancel).await;
                    return Ok(());
                }
            }
        }
    }
}

#[async_trait]
impl FlowNodeBehavior for ApprovalGateNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            let me = Arc::clone(&self);
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move {
                let Some(plan_id) = plan_id_of(&*msg.read().await) else {
                    return fail(node, msg, 1, "approval", missing_plan(), cancel).await;
                };
                let wallet = wallet_from_flow(&flow_of(node)?, &node.wallet, "payer")?;
                // Answer the HTTP request now; keep the original message for the send path.
                let pending = msg.deep_clone(false).await;
                node.report_status(
                    status(StatusFill::Yellow, StatusShape::Ring, "waiting for approval"),
                    cancel.clone(),
                )
                .await;
                node.fan_out_one(Envelope { port: 1, msg: pending }, cancel.clone()).await?;
                tokio::spawn(async move {
                    if let Err(err) = me.wait(msg, wallet, plan_id, cancel).await {
                        log::warn!("approval-gate stopped: {err}");
                    }
                });
                Ok(())
            })
            .await;
        }
    }
}

// ---------------------------------------------------------------------------------------
// rgb-approve: the human's approval, bound to plan digest + current flow revision

crate::node_hints!("rgb-approve", refs = ["wallet" => "rgb-wallet-ref"], caps = ["wallet", "network"], input = "object", outs = ["result" => "object"]);

#[flow_node("rgb-approve", red_name = "rgb-approve", inputs = 1, outputs = 1)]
struct RgbApproveNode {
    base: BaseFlowNodeState,
    wallet: String,
}

impl RgbApproveNode {
    fn build(
        _flow: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = WalletOnly::deserialize(&config.rest)?;
        Ok(Box::new(RgbApproveNode { base, wallet: require_wallet(&raw.wallet, "rgb-approve")? }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let flow = flow_of(self)?;
        let wallet = wallet_from_flow(&flow, &self.wallet, "payer")?;
        let input = payload_json(&*msg.read().await);
        let (plan_id, digest, actor) = (text(&input, "planId"), text(&input, "planDigest"), text(&input, "actor"));
        if plan_id.is_empty() || digest.is_empty() || actor.is_empty() {
            let err = CallError {
                status: 400,
                code: "invalid_approval".into(),
                message: "approval needs planId, planDigest and the approver's name".into(),
            };
            return fail(self, msg, 0, "approve", err, cancel).await;
        }
        let body = json!({ "plan_digest": digest, "flow_revision": flow_revision(&flow).await?, "actor": actor });
        match wallet.call(Method::POST, &format!("/plans/{plan_id}/approve"), Some(&body)).await {
            Ok(view) => {
                set_payload(&msg, json!({ "ok": true, "step": "approved", "payout": view }), None).await;
                self.report_status(status(StatusFill::Green, StatusShape::Dot, "approved"), cancel.clone()).await;
                self.fan_out_one(Envelope { port: 0, msg }, cancel).await
            }
            Err(err) => fail(self, msg, 0, "approve", err, cancel).await,
        }
    }
}

#[async_trait]
impl FlowNodeBehavior for RgbApproveNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move { node.handle(msg, cancel).await })
                .await;
        }
    }
}

// ---------------------------------------------------------------------------------------
// rgb-send-once: the only node that can cause a spend; the wallet journal makes it once

crate::node_hints!("rgb-send-once", refs = ["wallet" => "rgb-wallet-ref"], caps = ["wallet", "network", "spend"], input = "object", outs = ["sent" => "object", "error" => "object"]);

#[flow_node("rgb-send-once", red_name = "rgb-send-once", inputs = 1, outputs = 2)]
struct RgbSendOnceNode {
    base: BaseFlowNodeState,
    wallet: String,
}

impl RgbSendOnceNode {
    fn build(
        _flow: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = WalletOnly::deserialize(&config.rest)?;
        Ok(Box::new(RgbSendOnceNode { base, wallet: require_wallet(&raw.wallet, "rgb-send-once")? }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let flow = flow_of(self)?;
        let wallet = wallet_from_flow(&flow, &self.wallet, "payer")?;
        let Some(plan_id) = plan_id_of(&*msg.read().await) else {
            return fail(self, msg, 1, "send", missing_plan(), cancel).await;
        };
        // Same key for any retry of this node with this plan: the journal dedupes.
        let body = json!({
            "flow_revision": flow_revision(&flow).await?,
            "idempotency_key": format!("{plan_id}:{}", self.id()),
        });
        self.report_status(status(StatusFill::Yellow, StatusShape::Dot, "sending"), cancel.clone()).await;
        match wallet.call(Method::POST, &format!("/plans/{plan_id}/execute"), Some(&body)).await {
            Ok(view) => {
                let txid = text(&view, "txid");
                set_payload(&msg, json!({ "ok": true, "step": "sent", "payout": view }), None).await;
                self.report_status(
                    status(StatusFill::Green, StatusShape::Dot, format!("sent {}", &txid[..txid.len().min(10)])),
                    cancel.clone(),
                )
                .await;
                self.fan_out_one(Envelope { port: 0, msg }, cancel).await
            }
            Err(err) => fail(self, msg, 1, "send", err, cancel).await,
        }
    }
}

#[async_trait]
impl FlowNodeBehavior for RgbSendOnceNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move { node.handle(msg, cancel).await })
                .await;
        }
    }
}

// ---------------------------------------------------------------------------------------
// rgb-refresh: waits (in the background) until the wallet reports the payout SETTLED

#[derive(Debug, Deserialize)]
struct RefreshConfig {
    #[serde(default)]
    wallet: String,
    #[serde(default = "default_settle_timeout", rename = "timeoutSeconds", deserialize_with = "u64_or_str")]
    timeout_seconds: u64,
}

fn default_settle_timeout() -> u64 {
    1800
}

crate::node_hints!("rgb-refresh", refs = ["wallet" => "rgb-wallet-ref"], caps = ["wallet", "network"], input = "object", outs = ["settled" => "object"]);

#[flow_node("rgb-refresh", red_name = "rgb-refresh", inputs = 1, outputs = 1)]
struct RgbRefreshNode {
    base: BaseFlowNodeState,
    wallet: String,
    timeout: Duration,
}

impl RgbRefreshNode {
    fn build(
        _flow: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = RefreshConfig::deserialize(&config.rest)?;
        Ok(Box::new(RgbRefreshNode {
            base,
            wallet: require_wallet(&raw.wallet, "rgb-refresh")?,
            timeout: Duration::from_secs(raw.timeout_seconds.clamp(60, 7200)),
        }))
    }

    async fn wait(
        &self,
        msg: MsgHandle,
        wallet: WalletHandle,
        plan_id: String,
        cancel: CancellationToken,
    ) -> crate::Result<()> {
        let deadline = tokio::time::Instant::now() + self.timeout;
        while tokio::time::Instant::now() < deadline {
            tokio::select! {
                _ = cancel.cancelled() => return Err(N2linkError::TaskCancelled),
                _ = tokio::time::sleep(SETTLE_POLL) => {}
            }
            let Ok(view) = wallet.call(Method::GET, &format!("/plans/{plan_id}?refresh=true"), None).await else {
                continue; // indexer hiccup or rate limit: keep waiting
            };
            let state = text(&view, "state");
            self.report_status(status(StatusFill::Yellow, StatusShape::Ring, state.to_lowercase()), cancel.clone())
                .await;
            if state == "SETTLED" {
                set_payload(&msg, json!({ "ok": true, "step": "settled", "payout": view }), None).await;
                self.report_status(status(StatusFill::Green, StatusShape::Dot, "settled"), cancel.clone()).await;
                return self.fan_out_one(Envelope { port: 0, msg }, cancel).await;
            }
        }
        self.report_status(status(StatusFill::Red, StatusShape::Ring, "not settled yet; check status"), cancel).await;
        Ok(())
    }
}

#[async_trait]
impl FlowNodeBehavior for RgbRefreshNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            let me = Arc::clone(&self);
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move {
                let Some(plan_id) = plan_id_of(&*msg.read().await) else {
                    node.report_status(status(StatusFill::Red, StatusShape::Ring, "no plan"), cancel).await;
                    return Ok(());
                };
                let wallet = wallet_from_flow(&flow_of(node)?, &node.wallet, "payer")?;
                tokio::spawn(async move {
                    if let Err(err) = me.wait(msg, wallet, plan_id, cancel).await {
                        log::warn!("rgb-refresh stopped: {err}");
                    }
                });
                Ok(())
            })
            .await;
        }
    }
}

// ---------------------------------------------------------------------------------------
// rgb-receipt: the verifiable summary both sides can keep

crate::node_hints!("rgb-receipt", refs = ["wallet" => "rgb-wallet-ref"], input = "object", outs = ["receipt" => "object"]);

#[flow_node("rgb-receipt", red_name = "rgb-receipt", inputs = 1, outputs = 1)]
struct RgbReceiptNode {
    base: BaseFlowNodeState,
}

impl RgbReceiptNode {
    fn build(
        _flow: &Flow,
        base: BaseFlowNodeState,
        _config: &RedFlowNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        Ok(Box::new(RgbReceiptNode { base }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let payout = payload_json(&*msg.read().await).get("payout").cloned().unwrap_or(Value::Null);
        let pick = |key: &str| payout.get(key).cloned().unwrap_or(Value::Null);
        let receipt = json!({
            "planId": pick("plan_id"), "state": pick("state"), "network": pick("environment"),
            "assetId": pick("asset_id"), "assetTicker": pick("asset_ticker"),
            "amount": pick("amount_display"), "amountBaseUnits": pick("amount_base_units"),
            "recipient": pick("recipient_label"), "memo": pick("memo"),
            "txid": pick("txid"), "consignmentSha256": pick("consignment_sha256"),
            "approvedBy": pick("approved_by"), "planDigest": pick("plan_digest"),
            "flowRevision": pick("flow_revision"), "templateDigest": pick("template_digest"),
        });
        log::info!("PowerEdge receipt: {receipt}");
        set_payload(&msg, json!({ "ok": true, "step": "receipt", "receipt": receipt }), None).await;
        self.report_status(status(StatusFill::Green, StatusShape::Dot, "receipt"), cancel.clone()).await;
        self.fan_out_one(Envelope { port: 0, msg }, cancel).await
    }
}

#[async_trait]
impl FlowNodeBehavior for RgbReceiptNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move { node.handle(msg, cancel).await })
                .await;
        }
    }
}

// ---------------------------------------------------------------------------------------
// rgb-status: read-only payout status for the Track screen

crate::node_hints!("rgb-status", refs = ["wallet" => "rgb-wallet-ref"], caps = ["wallet", "network"], input = "object", outs = ["status" => "object"]);

#[flow_node("rgb-status", red_name = "rgb-status", inputs = 1, outputs = 1)]
struct RgbStatusNode {
    base: BaseFlowNodeState,
    wallet: String,
}

impl RgbStatusNode {
    fn build(
        _flow: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = WalletOnly::deserialize(&config.rest)?;
        Ok(Box::new(RgbStatusNode { base, wallet: require_wallet(&raw.wallet, "rgb-status")? }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let wallet = wallet_from_flow(&flow_of(self)?, &self.wallet, "payer")?;
        let plan_id = text(&payload_json(&*msg.read().await), "planId");
        let valid = plan_id.len() == 36 && plan_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if !valid {
            return fail(self, msg, 0, "status", missing_plan(), cancel).await;
        }
        match wallet.call(Method::GET, &format!("/plans/{plan_id}"), None).await {
            Ok(view) => {
                set_payload(&msg, json!({ "ok": true, "step": "status", "payout": view }), None).await;
                self.fan_out_one(Envelope { port: 0, msg }, cancel).await
            }
            Err(err) => fail(self, msg, 0, "status", err, cancel).await,
        }
    }
}

#[async_trait]
impl FlowNodeBehavior for RgbStatusNode {
    fn get_base(&self) -> &BaseFlowNodeState {
        &self.base
    }

    async fn run(self: Arc<Self>, stop_token: CancellationToken) {
        while !stop_token.is_cancelled() {
            let cancel = stop_token.child_token();
            with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move { node.handle(msg, cancel).await })
                .await;
        }
    }
}
