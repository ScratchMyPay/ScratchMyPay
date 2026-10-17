//! Tether WDK treasury nodes (read-only). Clients of `services/wdk-btc` on loopback.
//!
//! - `wdk-wallet-ref` (config): where the WDK treasury service listens.
//! - `wdk-balance`: treasury BTC balance → `msg.payload` and `msg.wdk.balance`.
//! - `wdk-fee-quote`: live fee rates × measured PowerEdge payout size. If `msg.wdk.balance`
//!   is present it also answers "can the treasury cover N payouts?" (`payload.coverage.enough`).
//! - `wdk-transfers`: recent treasury transfers.
//!
//! None of these can send; the service exposes no spending endpoint.

use std::sync::Arc;

use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::N2linkError;
use crate::runtime::egress::EgressPolicyHandle;
use crate::runtime::engine::Engine;
use crate::runtime::flow::Flow;
use crate::runtime::model::ElementId;
use crate::runtime::model::json::{RedFlowNodeConfig, RedGlobalNodeConfig};
use crate::runtime::model::{MsgHandle, Variant};
use crate::runtime::nodes::*;
use n2link_macro::*;

use super::client::{CallError, WalletHandle, loopback_url, read_token};
use super::{payload_json, require_wallet, set_payload, status, u64_or_str};

crate::node_hints!("wdk-wallet-ref", caps = ["wallet", "network"]);

#[global_node("wdk-wallet-ref", red_name = "wdk-wallet-ref")]
pub struct WdkWalletRefNode {
    base: BaseGlobalNodeState,
    base_url: String,
    token: String,
    egress: EgressPolicyHandle,
}

impl WdkWalletRefNode {
    fn build(
        engine: &Engine,
        config: &RedGlobalNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn GlobalNodeBehavior>> {
        let raw = config.rest.get("baseUrl").and_then(Value::as_str).unwrap_or_default().trim().to_owned();
        Ok(Box::new(WdkWalletRefNode {
            base: BaseGlobalNodeState {
                id: config.id,
                name: config.name.clone(),
                type_str: "wdk-wallet-ref",
                ordering: config.ordering,
                context: engine.get_context_manager().new_context(engine.context(), config.id.to_string()),
                disabled: config.disabled,
            },
            base_url: loopback_url(&raw)?,
            token: read_token()?,
            egress: engine.egress_policy().clone(),
        }))
    }
}

#[async_trait::async_trait]
impl GlobalNodeBehavior for WdkWalletRefNode {
    fn get_base(&self) -> &BaseGlobalNodeState {
        &self.base
    }
}

fn wdk_from_flow(flow: &Flow, reference: &str) -> crate::Result<WalletHandle> {
    let id: ElementId =
        reference.parse().map_err(|_| N2linkError::invalid_operation("WDK node reference is not a node id"))?;
    let engine = flow.engine().ok_or_else(|| N2linkError::invalid_operation("WDK node has no engine"))?;
    let global = engine
        .find_global_node_by_id(&id)
        .ok_or_else(|| N2linkError::invalid_operation(&format!("wdk-wallet-ref '{id}' was not loaded")))?;
    let node = global
        .as_any()
        .downcast_ref::<WdkWalletRefNode>()
        .ok_or_else(|| N2linkError::invalid_operation(&format!("node '{id}' is not a wdk-wallet-ref")))?;
    Ok(WalletHandle::new(node.base_url.clone(), node.token.clone(), node.egress.clone()))
}

/// Store a WDK result under `msg.wdk.<key>` (so later nodes can combine them) and in payload.
async fn put_wdk(msg: &MsgHandle, key: &str, value: Value, payload: Value) {
    let mut guard = msg.write().await;
    let mut wdk = guard
        .get("wdk")
        .and_then(|v| serde_json::to_value(v).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    wdk[key] = value;
    guard.set("wdk".to_owned(), Variant::from(wdk));
    guard.set("payload".to_owned(), Variant::from(payload));
}

async fn wdk_fail<N: FlowNodeBehavior + ?Sized>(
    node: &N,
    msg: MsgHandle,
    step: &str,
    err: CallError,
    cancel: CancellationToken,
) -> crate::Result<()> {
    set_payload(&msg, err.to_json(step), Some(err.status)).await;
    node.report_status(status(StatusFill::Red, StatusShape::Ring, err.code.clone()), cancel.clone()).await;
    node.fan_out_one(Envelope { port: 0, msg }, cancel).await
}

fn flow_of<N: FlowNodeBehavior + ?Sized>(node: &N) -> crate::Result<Flow> {
    node.flow().ok_or_else(|| N2linkError::invalid_operation("WDK node has no flow"))
}

macro_rules! wdk_run {
    ($ty:ty) => {
        #[async_trait]
        impl FlowNodeBehavior for $ty {
            fn get_base(&self) -> &BaseFlowNodeState {
                &self.base
            }

            async fn run(self: Arc<Self>, stop_token: CancellationToken) {
                while !stop_token.is_cancelled() {
                    let cancel = stop_token.child_token();
                    with_uow(
                        self.as_ref(),
                        cancel.child_token(),
                        |node, msg| async move { node.handle(msg, cancel).await },
                    )
                    .await;
                }
            }
        }
    };
}

#[derive(Debug, Deserialize)]
struct WalletOnly {
    #[serde(default)]
    wallet: String,
}

// ---------------------------------------------------------------- wdk-balance
crate::node_hints!("wdk-balance", refs = ["wallet" => "wdk-wallet-ref"], caps = ["wallet", "network"], input = "any", outs = ["balance" => "object"]);

#[flow_node("wdk-balance", red_name = "wdk-balance", inputs = 1, outputs = 1)]
struct WdkBalanceNode {
    base: BaseFlowNodeState,
    wallet: String,
}

/// Status text for the treasury: total (main + door revenue) when the service reports it.
fn balance_status(body: &serde_json::Value) -> String {
    match (body["totalSat"].as_u64(), body["doorSat"].as_u64()) {
        (Some(total), Some(door)) if door > 0 => format!("{total} sats (door {door})"),
        (Some(total), _) => format!("{total} sats"),
        _ => format!("{} sats", body["balanceSat"].as_u64().unwrap_or(0)),
    }
}

#[cfg(test)]
mod balance_status_tests {
    use super::balance_status;
    use serde_json::json;

    #[test]
    fn shows_total_with_door_revenue_and_falls_back_to_main() {
        assert_eq!(balance_status(&json!({"balanceSat": 100000, "doorSat": 18000, "totalSat": 118000})), "118000 sats (door 18000)");
        assert_eq!(balance_status(&json!({"balanceSat": 5, "doorSat": 0, "totalSat": 5})), "5 sats");
        assert_eq!(balance_status(&json!({"balanceSat": 7})), "7 sats");
    }
}

impl WdkBalanceNode {
    fn build(
        _f: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _o: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = WalletOnly::deserialize(&config.rest)?;
        Ok(Box::new(WdkBalanceNode { base, wallet: require_wallet(&raw.wallet, "wdk-balance")? }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let wdk = wdk_from_flow(&flow_of(self)?, &self.wallet)?;
        match wdk.call(Method::GET, "/balance", None).await {
            Ok(body) => {
                let text = balance_status(&body);
                put_wdk(&msg, "balance", body.clone(), json!({"ok": true, "step": "wdk-balance", "balance": body}))
                    .await;
                self.report_status(status(StatusFill::Green, StatusShape::Dot, text), cancel.clone()).await;
                self.fan_out_one(Envelope { port: 0, msg }, cancel).await
            }
            Err(err) => wdk_fail(self, msg, "wdk-balance", err, cancel).await,
        }
    }
}
wdk_run!(WdkBalanceNode);

// ---------------------------------------------------------------- wdk-fee-quote
#[derive(Debug, Deserialize)]
struct FeeConfig {
    #[serde(default)]
    wallet: String,
    #[serde(default = "one", rename = "payoutsTonight", deserialize_with = "u64_or_str")]
    payouts_tonight: u64,
    #[serde(default)]
    speed: String,
}

fn one() -> u64 {
    1
}

crate::node_hints!("wdk-fee-quote", refs = ["wallet" => "wdk-wallet-ref"], caps = ["wallet", "network"], input = "any", outs = ["quote" => "object"]);

#[flow_node("wdk-fee-quote", red_name = "wdk-fee-quote", inputs = 1, outputs = 1)]
struct WdkFeeQuoteNode {
    base: BaseFlowNodeState,
    wallet: String,
    count: u64,
    speed: String,
}

impl WdkFeeQuoteNode {
    fn build(
        _f: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _o: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = FeeConfig::deserialize(&config.rest)?;
        let speed = if raw.speed.trim().is_empty() { "normal".to_owned() } else { raw.speed.trim().to_owned() };
        if !matches!(speed.as_str(), "normal" | "fast") {
            return Err(N2linkError::invalid_operation("wdk-fee-quote speed must be normal or fast"));
        }
        Ok(Box::new(WdkFeeQuoteNode {
            base,
            wallet: require_wallet(&raw.wallet, "wdk-fee-quote")?,
            count: raw.payouts_tonight.clamp(1, 1000),
            speed,
        }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let wdk = wdk_from_flow(&flow_of(self)?, &self.wallet)?;
        let has_balance = {
            let guard = msg.read().await;
            guard.get("wdk").and_then(|v| serde_json::to_value(v).ok()).is_some_and(|w| w.get("balance").is_some())
        };
        let path = if has_balance {
            format!("/check?count={}&speed={}", self.count, self.speed)
        } else {
            format!("/fees?count={}", self.count)
        };
        match wdk.call(Method::GET, &path, None).await {
            Ok(body) => {
                let (payload, text, fill) = if has_balance {
                    let cov = body["coverage"].clone();
                    let enough = cov["enough"].as_bool().unwrap_or(false);
                    let text = if enough {
                        format!("enough for {} payouts", cov["payoutsCovered"])
                    } else {
                        format!("short {} sats", cov["shortSat"])
                    };
                    (
                        json!({"ok": true, "step": "wdk-check", "cost": body["cost"], "coverage": cov}),
                        text,
                        if enough { StatusFill::Green } else { StatusFill::Red },
                    )
                } else {
                    let row = &body["bySpeed"][&self.speed];
                    (
                        json!({"ok": true, "step": "wdk-fees", "cost": body}),
                        format!("≈ {} sats/payout", row["perPayoutSat"]),
                        StatusFill::Green,
                    )
                };
                put_wdk(&msg, "fees", payload["cost"].clone(), payload).await;
                self.report_status(status(fill, StatusShape::Dot, text), cancel.clone()).await;
                self.fan_out_one(Envelope { port: 0, msg }, cancel).await
            }
            Err(err) => wdk_fail(self, msg, "wdk-fee-quote", err, cancel).await,
        }
    }
}
wdk_run!(WdkFeeQuoteNode);

// ---------------------------------------------------------------- wdk-transfers
#[derive(Debug, Deserialize)]
struct TransfersConfig {
    #[serde(default)]
    wallet: String,
    #[serde(default = "ten", deserialize_with = "u64_or_str")]
    limit: u64,
}

fn ten() -> u64 {
    10
}

crate::node_hints!("wdk-transfers", refs = ["wallet" => "wdk-wallet-ref"], caps = ["wallet", "network"], input = "any", outs = ["transfers" => "object"]);

#[flow_node("wdk-transfers", red_name = "wdk-transfers", inputs = 1, outputs = 1)]
struct WdkTransfersNode {
    base: BaseFlowNodeState,
    wallet: String,
    limit: u64,
}

impl WdkTransfersNode {
    fn build(
        _f: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _o: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = TransfersConfig::deserialize(&config.rest)?;
        Ok(Box::new(WdkTransfersNode {
            base,
            wallet: require_wallet(&raw.wallet, "wdk-transfers")?,
            limit: raw.limit.clamp(1, 100),
        }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let wdk = wdk_from_flow(&flow_of(self)?, &self.wallet)?;
        let _ = payload_json(&*msg.read().await);
        match wdk.call(Method::GET, &format!("/transfers?limit={}", self.limit), None).await {
            Ok(body) => {
                let n = body["transfers"].as_array().map_or(0, Vec::len);
                put_wdk(&msg, "transfers", body.clone(), json!({"ok": true, "step": "wdk-transfers", "transfers": body["transfers"], "address": body["address"]})).await;
                self.report_status(
                    status(StatusFill::Green, StatusShape::Dot, format!("{n} transfers")),
                    cancel.clone(),
                )
                .await;
                self.fan_out_one(Envelope { port: 0, msg }, cancel).await
            }
            Err(err) => wdk_fail(self, msg, "wdk-transfers", err, cancel).await,
        }
    }
}
wdk_run!(WdkTransfersNode);

// ---------------------------------------------------------------- door-stats
crate::node_hints!("door-stats", refs = ["wallet" => "wdk-wallet-ref"], caps = ["wallet", "network"], input = "any", outs = ["door" => "object"]);

#[derive(Debug, Deserialize)]
struct DoorStatsConfig {
    #[serde(default)]
    wallet: String,
    #[serde(default = "a_day", deserialize_with = "u64_or_str")]
    hours: u64,
}

fn a_day() -> u64 {
    24
}

/// Door and treasury for the dashboard: passes sold, revenue, active passes, treasury total, passes per hour.
#[flow_node("door-stats", red_name = "door-stats", inputs = 1, outputs = 1)]
struct DoorStatsNode {
    base: BaseFlowNodeState,
    wallet: String,
    hours: u64,
}

impl DoorStatsNode {
    fn build(
        _f: &Flow,
        base: BaseFlowNodeState,
        config: &RedFlowNodeConfig,
        _o: Option<&config::Config>,
    ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = DoorStatsConfig::deserialize(&config.rest)?;
        Ok(Box::new(DoorStatsNode { base, wallet: require_wallet(&raw.wallet, "door-stats")?, hours: raw.hours.clamp(1, 168) }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let wdk = wdk_from_flow(&flow_of(self)?, &self.wallet)?;
        let list = match wdk.call(Method::GET, "/door/passes?limit=500", None).await {
            Ok(v) => v,
            Err(err) => return wdk_fail(self, msg, "door-stats", err, cancel).await,
        };
        let balance = wdk.call(Method::GET, "/balance", None).await.ok();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let passes = list["passes"].as_array().cloned().unwrap_or_default();
        let sold = balance.as_ref().map(|b| b["door"]["passes"].clone()).unwrap_or_else(|| json!(passes.len()));
        let payload = json!({
            "passes": sold,
            "activePasses": list["activePasses"],
            "revenueSat": list["revenueSat"],
            "priceSat": list["priceSat"],
            "treasury": balance.as_ref().map(|b| b["treasury"].clone()),
            "perHour": super::stats::passes_per_hour(&passes, self.hours as i64, now),
        });
        let text = format!(
            "{} passes · {} sats",
            sold,
            super::stats::thousands(list["revenueSat"].as_f64().unwrap_or(0.0))
        );
        put_wdk(&msg, "door", payload.clone(), payload).await;
        self.report_status(status(StatusFill::Green, StatusShape::Dot, text), cancel.clone()).await;
        self.fan_out_one(Envelope { port: 0, msg }, cancel).await
    }
}
wdk_run!(DoorStatsNode);
