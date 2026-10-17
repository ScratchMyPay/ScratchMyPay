//! Dashboard data nodes (read-only): `rgb-funds`, `payout-stats`, `door-stats`.
//!
//! They shape wallet and treasury data for the `ui-*` widgets: balances, per-hour bar series, a recent
//! payouts table. `rgb-funds` and `payout-stats` reach the wallets through the addresses n2link was started
//! with (POWEREDGE_PAYER_URL / POWEREDGE_RECIPIENT_URL, loopback only) instead of a wallet reference, so a
//! library template that uses them is never ambiguous between the organizer and crew wallets.
//! None of these can plan, approve or send.

use std::sync::Arc;

use reqwest::Method;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::N2linkError;
use crate::runtime::flow::Flow;
use crate::runtime::model::MsgHandle;
use crate::runtime::model::json::RedFlowNodeConfig;
use crate::runtime::nodes::*;
use n2link_macro::*;

use super::client::{CallError, WalletHandle, loopback_url, read_token};
use super::{set_payload, status};

fn wallet_handle(flow: &Flow, which: &str) -> crate::Result<WalletHandle> {
    let (var, default) = if which == "crew" {
        ("POWEREDGE_RECIPIENT_URL", "http://127.0.0.1:8712")
    } else {
        ("POWEREDGE_PAYER_URL", "http://127.0.0.1:8711")
    };
    let base = std::env::var(var).unwrap_or_else(|_| default.to_owned());
    let engine = flow.engine().ok_or_else(|| N2linkError::invalid_operation("dashboard node has no engine"))?;
    Ok(WalletHandle::new(loopback_url(&base)?, read_token()?, engine.egress_policy().clone()))
}

fn flow_of<N: FlowNodeBehavior + ?Sized>(node: &N) -> crate::Result<Flow> {
    node.flow().ok_or_else(|| N2linkError::invalid_operation("dashboard node has no flow"))
}

async fn failed<N: FlowNodeBehavior + ?Sized>(node: &N, msg: MsgHandle, step: &str, err: CallError, cancel: CancellationToken) -> crate::Result<()> {
    set_payload(&msg, err.to_json(step), Some(err.status)).await;
    node.report_status(status(StatusFill::Red, StatusShape::Ring, err.code.clone()), cancel.clone()).await;
    node.fan_out_one(Envelope { port: 0, msg }, cancel).await
}

/// 12345678 -> "12,345,678"
pub(crate) fn thousands(n: f64) -> String {
    let rounded = if n.fract().abs() < 1e-9 { format!("{n:.0}") } else { format!("{n:.2}") };
    let (int, frac) = rounded.split_once('.').map(|(i, f)| (i.to_owned(), format!(".{f}"))).unwrap_or((rounded.clone(), String::new()));
    let (sign, digits) = int.strip_prefix('-').map(|d| ("-", d)).unwrap_or(("", int.as_str()));
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}{out}{frac}")
}

/// Per-hour bars from `[{hour, <key>: n, volume: {T: x}}]`: labels in epoch ms, one series per name.
pub(crate) fn hourly_series(buckets: &[Value], keys: &[&str]) -> Value {
    let labels: Vec<Value> = buckets.iter().map(|b| json!(b["hour"].as_i64().unwrap_or(0) * 1000)).collect();
    let mut series = Map::new();
    for key in keys {
        series.insert((*key).to_owned(), Value::Array(buckets.iter().map(|b| json!(b[*key].as_f64().unwrap_or(0.0))).collect()));
    }
    json!({ "labels": labels, "series": series })
}

fn volume_series(buckets: &[Value]) -> Value {
    let mut tickers: Vec<String> = buckets
        .iter()
        .filter_map(|b| b["volume"].as_object())
        .flat_map(|m| m.keys().cloned())
        .collect();
    tickers.sort();
    tickers.dedup();
    let labels: Vec<Value> = buckets.iter().map(|b| json!(b["hour"].as_i64().unwrap_or(0) * 1000)).collect();
    let mut series = Map::new();
    for t in tickers {
        let values = buckets.iter().map(|b| json!(b["volume"][&t].as_f64().unwrap_or(0.0))).collect();
        series.insert(t, Value::Array(values));
    }
    json!({ "labels": labels, "series": series })
}

/// Bucket door passes (issuedAt in ms) into the last `hours` hours.
pub(crate) fn passes_per_hour(passes: &[Value], hours: i64, now_ms: i64) -> Value {
    let start_hour = now_ms / 3_600_000 - hours + 1;
    let mut counts = vec![0u64; hours as usize];
    for p in passes {
        if let Some(t) = p["issuedAt"].as_i64() {
            let i = t / 3_600_000 - start_hour;
            if (0..hours).contains(&i) {
                counts[i as usize] += 1;
            }
        }
    }
    let labels: Vec<Value> = (0..hours).map(|i| json!((start_hour + i) * 3_600_000)).collect();
    json!({ "labels": labels, "series": { "passes": counts } })
}

macro_rules! run_impl {
    ($ty:ty) => {
        #[async_trait]
        impl FlowNodeBehavior for $ty {
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
    };
}

// ---------------------------------------------------------------- rgb-funds
crate::node_hints!("rgb-funds", caps = ["wallet", "network"], input = "any", outs = ["funds" => "object"]);

#[derive(Debug, Deserialize)]
struct FundsConfig {
    #[serde(default)]
    wallet: String,
}

#[flow_node("rgb-funds", red_name = "rgb-funds", inputs = 1, outputs = 1)]
struct RgbFundsNode {
    base: BaseFlowNodeState,
    which: String,
}

impl RgbFundsNode {
    fn build(_f: &Flow, base: BaseFlowNodeState, config: &RedFlowNodeConfig, _o: Option<&config::Config>) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let raw = FundsConfig::deserialize(&config.rest)?;
        let which = if raw.wallet.is_empty() { "organizer".to_owned() } else { raw.wallet };
        if which != "organizer" && which != "crew" {
            return Err(N2linkError::invalid_operation("rgb-funds: wallet must be organizer or crew"));
        }
        Ok(Box::new(RgbFundsNode { base, which }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let wallet = wallet_handle(&flow_of(self)?, &self.which)?;
        let assets = match wallet.call(Method::GET, "/assets", None).await {
            Ok(v) => v,
            Err(err) => return failed(self, msg, "funds", err, cancel).await,
        };
        let mut by_ticker = Map::new();
        for a in assets["assets"].as_array().cloned().unwrap_or_default() {
            let precision = a["precision"].as_i64().unwrap_or(0) as i32;
            let spendable = a["spendable"].as_f64().unwrap_or(0.0) / 10f64.powi(precision);
            if let Some(t) = a["ticker"].as_str() {
                by_ticker.insert(t.to_owned(), json!(spendable));
            }
        }
        // Bitcoin side (sats). The organizer's /funds also estimates how many payouts it can still afford.
        let funds = wallet.call(Method::GET, "/funds?fee_rate=2", None).await.ok();
        let plain = funds.as_ref().and_then(|f| f["btc"]["vanilla"]["spendable"].as_f64());
        let colored = funds.as_ref().and_then(|f| f["btc"]["colored"]["spendable"].as_f64());
        let payload = json!({
            "wallet": self.which,
            "assets": by_ticker,
            "sats": { "plain": plain, "colored": colored, "total": plain.zip(colored).map(|(p, c)| p + c) },
            "payoutsPossible": funds.as_ref().map(|f| f["estimate"]["payouts_from_colored_sats"].clone()),
            "freeOutputs": funds.as_ref().map(|f| f["utxos"]["free_colorable"].clone()),
        });
        let text = by_ticker
            .iter()
            .map(|(t, v)| format!("{t} {}", thousands(v.as_f64().unwrap_or(0.0))))
            .chain(plain.zip(colored).map(|(p, c)| format!("{} sats", thousands(p + c))))
            .collect::<Vec<_>>()
            .join(" · ");
        set_payload(&msg, payload, None).await;
        self.report_status(status(StatusFill::Green, StatusShape::Dot, text), cancel.clone()).await;
        self.fan_out_one(Envelope { port: 0, msg }, cancel).await
    }
}
run_impl!(RgbFundsNode);

// ---------------------------------------------------------------- payout-stats
crate::node_hints!("payout-stats", caps = ["wallet", "network"], input = "any", outs = ["stats" => "object"]);

#[flow_node("payout-stats", red_name = "payout-stats", inputs = 1, outputs = 1)]
struct PayoutStatsNode {
    base: BaseFlowNodeState,
    hours: i64,
    limit: i64,
}

impl PayoutStatsNode {
    fn build(_f: &Flow, base: BaseFlowNodeState, config: &RedFlowNodeConfig, _o: Option<&config::Config>) -> crate::Result<Box<dyn FlowNodeBehavior>> {
        let num = |k: &str, d: i64| match config.rest.get(k) {
            Some(Value::Number(n)) => n.as_i64().unwrap_or(d),
            Some(Value::String(s)) => s.trim().parse().unwrap_or(d),
            _ => d,
        };
        Ok(Box::new(PayoutStatsNode { base, hours: num("hours", 24).clamp(1, 168), limit: num("limit", 10).clamp(1, 50) }))
    }

    async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
        let wallet = wallet_handle(&flow_of(self)?, "organizer")?;
        let stats = match wallet.call(Method::GET, &format!("/stats?hours={}", self.hours), None).await {
            Ok(v) => v,
            Err(err) => return failed(self, msg, "payout-stats", err, cancel).await,
        };
        let recent = wallet.call(Method::GET, &format!("/plans?limit={}", self.limit), None).await.ok();
        let buckets = stats["buckets"].as_array().cloned().unwrap_or_default();
        let rows: Vec<Value> = recent
            .as_ref()
            .and_then(|r| r["plans"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .map(|p| {
                json!({
                    "time": p["created_at"].as_f64().map(|t| (t * 1000.0) as i64),
                    "amount": format!("{} {}", p["amount_display"].as_str().unwrap_or("?"), p["asset_ticker"].as_str().unwrap_or("")),
                    "to": p["recipient_label"], "memo": p["memo"], "state": p["state"], "txid": p["txid"],
                })
            })
            .collect();
        let totals = &stats["totals"];
        let p50 = stats["settleSeconds"]["p50"].clone();
        let payload = json!({
            "stats": stats,
            "recent": rows,
            "chart": hourly_series(&buckets, &["settled", "failed"]),
            "volumeChart": volume_series(&buckets),
            "settled": totals["settled"], "failed": totals["failed"], "inFlight": totals["inFlight"],
            "settleP50": p50, "settleP90": stats["settleSeconds"]["p90"],
        });
        let text = format!(
            "{} settled · {} failed · p50 {}",
            totals["settled"], totals["failed"],
            p50.as_f64().map(|s| format!("{s:.0} s")).unwrap_or_else(|| "—".into())
        );
        set_payload(&msg, payload, None).await;
        self.report_status(status(StatusFill::Green, StatusShape::Dot, text), cancel.clone()).await;
        self.fan_out_one(Envelope { port: 0, msg }, cancel).await
    }
}
run_impl!(PayoutStatsNode);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_formats_ints_and_decimals() {
        assert_eq!(thousands(70540.0), "70,540");
        assert_eq!(thousands(999892.0), "999,892");
        assert_eq!(thousands(93.5), "93.50");
        assert_eq!(thousands(-1234.0), "-1,234");
        assert_eq!(thousands(12.0), "12");
    }

    #[test]
    fn hourly_and_volume_series_line_up_with_buckets() {
        let buckets = vec![
            json!({"hour": 3600, "settled": 2, "failed": 0, "volume": {"USDT": 2.0}}),
            json!({"hour": 7200, "settled": 1, "failed": 1, "volume": {"TUSDT": 1.0}}),
        ];
        let s = hourly_series(&buckets, &["settled", "failed"]);
        assert_eq!(s["labels"], json!([3_600_000, 7_200_000]));
        assert_eq!(s["series"]["settled"], json!([2.0, 1.0]));
        let v = volume_series(&buckets);
        assert_eq!(v["series"]["USDT"], json!([2.0, 0.0]));
        assert_eq!(v["series"]["TUSDT"], json!([0.0, 1.0]));
    }

    #[test]
    fn passes_are_bucketed_into_the_last_hours() {
        let now = 10 * 3_600_000 + 60_000; // one minute into hour 10
        let passes = vec![json!({"issuedAt": now - 10}), json!({"issuedAt": now - 3_600_000}), json!({"issuedAt": 1})];
        let s = passes_per_hour(&passes, 3, now);
        assert_eq!(s["series"]["passes"], json!([0, 1, 1]));
    }
}
