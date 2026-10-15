//! RGB2link PowerEdge deploy gate: a flow compiled from the pinned `rgb-payment-v1`
//! template must be accepted by n2link's own loader (`Engine::prepare_flows`), and a
//! tampered copy must not get past the PowerEdge validator first.

#![cfg(feature = "nodes_rgb")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use n2link_core::runtime::engine::Engine;
use n2link_core::runtime::registry::RegistryBuilder;
use poweredge::{AssetInfo, BindContext, Environment, TemplateRegistry, WorkflowIntent, compile, validate};
use serde_json::{Value, json};

const WORKSPACE: &str = "5a1e5a1e5a1e5a1e";

fn compiled() -> (TemplateRegistry, poweredge::CompiledFlow) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../templates");
    let registry = TemplateRegistry::load_dir(&root).expect("pinned templates load");
    let intent = WorkflowIntent::parse(
        json!({
            "version": 1, "template_id": "rgb-payment-v1", "template_version": "1.0.0",
            "environment": "mutinynet",
            "parameters": {"recipient_label": "sound engineer", "amount": "25",
                           "asset_ticker": "tUSDT", "memo": "Saturday's show"}
        })
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    let ctx = BindContext {
        workspace_id: WORKSPACE.into(),
        values: BTreeMap::from([
            ("payer_wallet_url".into(), "http://127.0.0.1:8711".into()),
            ("recipient_wallet_url".into(), "http://127.0.0.1:8712".into()),
            ("provenance".into(), "Drafted by local AI (test fixture).".into()),
        ]),
        assets: BTreeMap::from([("TUSDT".into(), AssetInfo { precision: 6 })]),
    };
    let flow = compile(&registry, &intent, &ctx).expect("hero intent compiles");
    (registry, flow)
}

fn with_tab(nodes: Vec<Value>) -> Value {
    let mut flows = vec![json!({"id": WORKSPACE, "type": "tab", "label": "PowerEdge payouts"})];
    flows.extend(nodes);
    Value::Array(flows)
}

#[tokio::test]
async fn compiled_payout_flow_loads_in_the_n2link_engine() {
    let token = std::env::temp_dir().join(format!("poweredge-token-{}", std::process::id()));
    std::fs::write(&token, "t".repeat(64)).unwrap();
    // SAFETY: set before the engine reads it; no other test in this binary reads this variable.
    unsafe { std::env::set_var("POWEREDGE_WALLET_TOKEN_FILE", &token) };

    let (_, flow) = compiled();
    let registry = RegistryBuilder::default().build().unwrap();
    let result = Engine::prepare_flows(&with_tab(flow.nodes), &registry, None);
    std::fs::remove_file(&token).ok();
    result.expect("n2link accepts the compiled PowerEdge flow");
}

#[test]
fn a_bypass_edit_is_caught_before_deploy() {
    let (registry, flow) = compiled();
    let manifest = &registry.resolve("rgb-payment-v1", "1.0.0").unwrap().manifest;
    let send_id = flow.nodes.iter().find(|n| n["type"] == "rgb-send-once").unwrap()["id"].clone();
    let mut edited = flow.nodes.clone();
    for node in &mut edited {
        if node["type"] == "rgb-plan" {
            node["wires"] = json!([[send_id], []]);
        }
    }
    assert!(validate(manifest, &edited, Environment::Mutinynet).is_err());
}
