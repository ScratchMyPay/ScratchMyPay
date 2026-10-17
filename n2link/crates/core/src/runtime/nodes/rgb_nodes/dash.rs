//! Dashboard widget nodes (sinks): `ui-stat`, `ui-chart`, `ui-table`.
//!
//! Each node keeps its latest value (and, for an appending chart, a short history) in an in-memory
//! registry inside n2link. The guided app's dashboard page (`/poweredge-app/dashboard`, behind the door)
//! reads `dashboard_snapshot()` through `/poweredge-app/api/dashboard`. Widgets disappear when their node
//! is removed or redeployed away. Nothing here reads a wallet or sends anything: widgets only display.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};

use crate::runtime::flow::Flow;
use crate::runtime::model::MsgHandle;
use crate::runtime::model::json::RedFlowNodeConfig;
use crate::runtime::nodes::*;
use n2link_macro::*;

use super::status;

const DEFAULT_POINTS: usize = 288;

#[derive(Clone, Debug)]
struct Widget {
    kind: &'static str,
    title: String,
    group: String,
    order: i64,
    unit: String,
    mode: String,
    columns: Vec<(String, String)>,
    value: Value,
    sub: Value,
    history: VecDeque<(i64, f64)>,
    cap: usize,
    updated_at: i64,
}

fn registry() -> &'static Mutex<BTreeMap<String, Widget>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<String, Widget>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Everything the dashboard page needs, ordered by group, then order, then title.
pub fn dashboard_snapshot() -> Value {
    let reg = registry().lock().expect("dashboard registry");
    let mut widgets: Vec<&Widget> = reg.values().collect();
    widgets.sort_by(|a, b| (a.group.as_str(), a.order, a.title.as_str()).cmp(&(b.group.as_str(), b.order, b.title.as_str())));
    let list: Vec<Value> = widgets
        .into_iter()
        .map(|w| {
            json!({
                "kind": w.kind, "title": w.title, "group": w.group, "order": w.order, "unit": w.unit,
                "mode": w.mode, "value": w.value, "sub": w.sub, "updatedAt": w.updated_at,
                "columns": w.columns.iter().map(|(l, k)| json!({"label": l, "key": k})).collect::<Vec<_>>(),
                "history": w.history.iter().map(|(t, v)| json!([t, v])).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "widgets": list, "now": now_ms() })
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn int_of(v: &Value, default: i64) -> i64 {
    match v {
        Value::Number(n) => n.as_i64().unwrap_or(default),
        Value::String(s) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

fn number_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

/// `payload.totals.settled` on a message (first segment is a top-level message property).
async fn msg_path(msg: &MsgHandle, path: &str) -> Value {
    let path = path.trim();
    if path.is_empty() {
        return Value::Null;
    }
    let mut parts = path.split('.');
    let root = parts.next().unwrap_or("payload");
    let guard = msg.read().await;
    let mut value = guard.get(root).and_then(|v| serde_json::to_value(v).ok()).unwrap_or(Value::Null);
    for part in parts {
        value = match value {
            Value::Object(mut map) => map.remove(part).unwrap_or(Value::Null),
            Value::Array(mut items) => part.parse::<usize>().ok().filter(|i| *i < items.len()).map(|i| items.swap_remove(i)).unwrap_or(Value::Null),
            _ => Value::Null,
        };
    }
    value
}

fn parse_columns(spec: &str) -> Vec<(String, String)> {
    spec.split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| match c.split_once(':') {
            Some((label, key)) => (label.trim().to_owned(), key.trim().to_owned()),
            None => (c.to_owned(), c.to_owned()),
        })
        .collect()
}

fn short(text: &str, max: usize) -> String {
    if text.chars().count() <= max { text.to_owned() } else { format!("{}…", text.chars().take(max).collect::<String>()) }
}

/// Apply one message to a widget; returns the status text for the node.
fn apply(w: &mut Widget, value: Value, sub: Value, now: i64) -> String {
    w.updated_at = now;
    match (w.kind, w.mode.as_str()) {
        ("chart", "append") => {
            if let Some(x) = number_of(&value) {
                w.history.push_back((now, x));
                while w.history.len() > w.cap {
                    w.history.pop_front();
                }
            }
            w.value = value;
            format!("{} points", w.history.len())
        }
        ("chart", _) => {
            let bars = w.value_labels(&value);
            w.value = value;
            format!("{bars} bars")
        }
        ("table", _) => {
            let rows = value.as_array().map(Vec::len).unwrap_or(0);
            w.value = value;
            format!("{rows} rows")
        }
        _ => {
            let shown = short(format!("{} {}", text_of(&value), w.unit).trim(), 40);
            w.value = value;
            w.sub = sub;
            shown
        }
    }
}

impl Widget {
    fn value_labels(&self, value: &Value) -> usize {
        value["labels"].as_array().map(Vec::len).unwrap_or(0)
    }
}

// ---------------------------------------------------------------- the three widget nodes
macro_rules! ui_node {
    ($name:literal, $ty:ident, $kind:literal) => {
        crate::node_hints!($name, caps = ["dashboard"], input = "any");

        #[flow_node($name, red_name = $name, inputs = 1, outputs = 0)]
        struct $ty {
            base: BaseFlowNodeState,
            id: String,
            template: Widget,
            property: String,
            sub_property: String,
        }

        impl $ty {
            fn build(
                _f: &Flow,
                base: BaseFlowNodeState,
                config: &RedFlowNodeConfig,
                _o: Option<&config::Config>,
            ) -> crate::Result<Box<dyn FlowNodeBehavior>> {
                let c = &config.rest;
                let get = |k: &str| c.get(k).cloned().unwrap_or(Value::Null);
                let mode = text_of(&get("mode"));
                let template = Widget {
                    kind: $kind,
                    title: { let t = text_of(&get("title")); if t.is_empty() { config.name.clone() } else { t } },
                    group: { let g = text_of(&get("group")); if g.is_empty() { "Dashboard".into() } else { g } },
                    order: int_of(&get("order"), 0),
                    unit: text_of(&get("unit")),
                    mode: if $kind == "chart" && mode != "append" { "replace".into() } else { mode },
                    columns: parse_columns(&text_of(&get("columns"))),
                    value: Value::Null,
                    sub: Value::Null,
                    history: VecDeque::new(),
                    cap: int_of(&get("points"), DEFAULT_POINTS as i64).clamp(2, 2000) as usize,
                    updated_at: 0,
                };
                let property = { let p = text_of(&get("property")); if p.is_empty() { "payload".into() } else { p } };
                let sub_property = text_of(&get("subProperty"));
                Ok(Box::new($ty { id: base.id.to_string(), base, template, property, sub_property }))
            }

            async fn handle(&self, msg: MsgHandle, cancel: CancellationToken) -> crate::Result<()> {
                let value = msg_path(&msg, &self.property).await;
                let sub = msg_path(&msg, &self.sub_property).await;
                let text = {
                    let mut reg = registry().lock().expect("dashboard registry");
                    let w = reg.entry(self.id.clone()).or_insert_with(|| self.template.clone());
                    apply(w, value, sub, now_ms())
                };
                self.report_status(status(StatusFill::Green, StatusShape::Dot, text), cancel).await;
                Ok(())
            }
        }

        #[async_trait]
        impl FlowNodeBehavior for $ty {
            fn get_base(&self) -> &BaseFlowNodeState {
                &self.base
            }

            async fn run(self: Arc<Self>, stop_token: CancellationToken) {
                registry().lock().expect("dashboard registry").insert(self.id.clone(), self.template.clone());
                while !stop_token.is_cancelled() {
                    let cancel = stop_token.child_token();
                    with_uow(self.as_ref(), cancel.child_token(), |node, msg| async move { node.handle(msg, cancel).await })
                        .await;
                }
                registry().lock().expect("dashboard registry").remove(&self.id);
            }
        }
    };
}

ui_node!("ui-stat", UiStatNode, "stat");
ui_node!("ui-chart", UiChartNode, "chart");
ui_node!("ui-table", UiTableNode, "table");

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(kind: &'static str, mode: &str) -> Widget {
        Widget {
            kind, title: "t".into(), group: "g".into(), order: 0, unit: "sats".into(), mode: mode.into(),
            columns: vec![], value: Value::Null, sub: Value::Null, history: VecDeque::new(), cap: 3, updated_at: 0,
        }
    }

    #[test]
    fn appending_chart_keeps_only_the_last_points_and_ignores_non_numbers() {
        let mut w = widget("chart", "append");
        for (i, v) in [json!(1), json!("2"), json!("x"), json!(3), json!(4)].into_iter().enumerate() {
            apply(&mut w, v, Value::Null, i as i64);
        }
        assert_eq!(w.history.iter().map(|(_, v)| *v).collect::<Vec<_>>(), vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn stat_shows_value_with_unit_and_table_counts_rows() {
        let mut s = widget("stat", "");
        assert_eq!(apply(&mut s, json!(70540), json!("plain + RGB"), 1), "70540 sats");
        assert_eq!(s.sub, json!("plain + RGB"));
        let mut t = widget("table", "");
        assert_eq!(apply(&mut t, json!([{"a": 1}, {"a": 2}]), Value::Null, 1), "2 rows");
    }

    #[test]
    fn columns_parse_label_key_pairs() {
        assert_eq!(
            parse_columns("Amount:amount, To:to,state"),
            vec![("Amount".into(), "amount".into()), ("To".into(), "to".into()), ("state".into(), "state".into())]
        );
    }
}
