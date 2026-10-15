//! End-to-end checks against the real pinned template in `templates/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use poweredge::{compile, validate, AssetInfo, BindContext, Environment, Error, TemplateRegistry, WorkflowIntent};
use serde_json::{json, Value};

const WORKSPACE: &str = "a1b2c3d4e5f60718";

fn templates_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../templates")
}

fn registry() -> TemplateRegistry {
    TemplateRegistry::load_dir(&templates_root()).expect("pinned templates load")
}

fn ctx() -> BindContext {
    BindContext {
        workspace_id: WORKSPACE.into(),
        values: BTreeMap::from([
            ("payer_wallet_url".into(), "http://127.0.0.1:8711".into()),
            ("recipient_wallet_url".into(), "http://127.0.0.1:8712".into()),
        ]),
        assets: BTreeMap::from([("TUSDT".into(), AssetInfo { precision: 6 })]),
    }
}

fn hero_json() -> Value {
    json!({
        "version": 1, "template_id": "rgb-payment-v1", "template_version": "1.0.0",
        "environment": "mutinynet",
        "parameters": { "recipient_label": "sound engineer", "amount": "25",
                        "asset_ticker": "tUSDT", "memo": "Saturday's show" },
        "rationale": "Pay the sound engineer for Saturday."
    })
}

fn intent(value: &Value) -> poweredge::Result<WorkflowIntent> {
    WorkflowIntent::parse(value.to_string().as_bytes())
}

fn hero() -> WorkflowIntent {
    intent(&hero_json()).unwrap()
}

fn node<'a>(nodes: &'a [Value], node_type: &str) -> &'a Value {
    nodes.iter().find(|n| n["type"] == node_type).unwrap_or_else(|| panic!("{node_type}"))
}

#[test]
fn pinned_template_loads_with_verified_digest() {
    let registry = registry();
    let template = registry.resolve("rgb-payment-v1", "1.0.0").unwrap();
    assert_eq!(template.digest().len(), 64);
    let catalog = registry.catalog().to_string();
    assert!(catalog.contains("rgb-payment-v1") && !catalog.contains("baseUrl"), "catalog leaks graph");
}

#[test]
fn hero_compiles_deterministically() {
    let registry = registry();
    let a = compile(&registry, &hero(), &ctx()).unwrap();
    let b = compile(&registry, &hero(), &ctx()).unwrap();
    assert_eq!(a, b);
    assert_eq!(serde_json::to_vec(&a.nodes).unwrap(), serde_json::to_vec(&b.nodes).unwrap());

    let mut reworded = hero();
    reworded.rationale = Some("Different explanation".into());
    assert_eq!(compile(&registry, &reworded, &ctx()).unwrap().flow_digest, a.flow_digest);

    let mut other_tab = ctx();
    other_tab.workspace_id = "00000000000000ff".into();
    assert_ne!(compile(&registry, &hero(), &other_tab).unwrap().flow_digest, a.flow_digest);
}

#[test]
fn hero_binds_amount_with_wallet_precision_and_resolves_references() {
    let flow = compile(&registry(), &hero(), &ctx()).unwrap();
    assert_eq!(flow.bound["amount"], json!("25"));
    assert_eq!(flow.bound["amount_base_units"], json!(25_000_000u64));
    // "tUSDT" as typed by a person resolves to the wallet's uppercase RGB ticker.
    assert_eq!(flow.bound["asset_ticker"], json!("TUSDT"));

    let plan = node(&flow.nodes, "rgb-plan");
    assert_eq!(plan["amountBaseUnits"], json!(25_000_000u64));
    assert_eq!(plan["recipientLabel"], json!("sound engineer"));
    assert_eq!(plan["z"], json!(WORKSPACE));

    let ids: Vec<&str> = flow.nodes.iter().map(|n| n["id"].as_str().unwrap()).collect();
    assert!(ids.iter().all(|id| id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())));
    assert!(flow.nodes.iter().all(|n| n.get("ref").is_none()));
    assert!(ids.contains(&plan["wallet"].as_str().unwrap()), "config reference resolved");
    assert_eq!(node(&flow.nodes, "rgb-wallet-ref")["environment"], json!("mutinynet"));
}

#[test]
fn integer_amount_from_the_model_is_rejected() {
    // Exactly what local QVAC produced in the 2026-10-15 smoke test (R-AMOUNT).
    let mut value = hero_json();
    value["parameters"]["amount"] = json!(25_000_000);
    let err = compile(&registry(), &intent(&value).unwrap(), &ctx()).unwrap_err();
    assert_eq!(err.code(), "invalid_parameter", "{err}");
}

#[test]
fn bad_parameters_fail_closed() {
    let cases: Vec<(&str, Value)> = vec![
        ("parameters", json!({"recipient_label": "x", "amount": "25", "asset_ticker": "tUSDT", "node_type": "exec"})),
        ("parameters", json!({"amount": "25", "asset_ticker": "tUSDT"})),
        ("parameters", json!({"recipient_label": "x", "amount": "25", "asset_ticker": "BTC"})),
        ("parameters", json!({"recipient_label": "x", "amount": "25.0000001", "asset_ticker": "tUSDT"})),
        ("parameters", json!({"recipient_label": "x", "amount": "-5", "asset_ticker": "tUSDT"})),
        ("parameters", json!({"recipient_label": "x".repeat(65), "amount": "25", "asset_ticker": "tUSDT"})),
    ];
    for (field, params) in cases {
        let mut value = hero_json();
        value[field] = params.clone();
        let err = compile(&registry(), &intent(&value).unwrap(), &ctx()).unwrap_err();
        assert_eq!(err.code(), "invalid_parameter", "{params}: {err}");
    }
}

#[test]
fn model_text_cannot_inject_placeholders() {
    let mut value = hero_json();
    value["parameters"]["memo"] = json!("{{ctx:payer_wallet_url}}");
    let err = compile(&registry(), &intent(&value).unwrap(), &ctx()).unwrap_err();
    assert_eq!(err.code(), "unsafe_graph", "{err}");
}

#[test]
fn unknown_template_or_version_has_no_fallback() {
    for (id, version) in [("rgb-payment-v2", "1.0.0"), ("rgb-payment-v1", "1.0.1"), ("rgb-payment-v1", "1.1.0")] {
        let mut value = hero_json();
        value["template_id"] = json!(id);
        value["template_version"] = json!(version);
        let err = compile(&registry(), &intent(&value).unwrap(), &ctx()).unwrap_err();
        assert!(matches!(err, Error::UnknownTemplate { .. }), "{id}@{version}: {err}");
    }
}

#[test]
fn missing_operator_context_is_rejected() {
    let mut context = ctx();
    context.values.remove("payer_wallet_url");
    assert_eq!(compile(&registry(), &hero(), &context).unwrap_err().code(), "missing_context");
    let mut context = ctx();
    context.workspace_id = "not-a-tab-id".into();
    assert_eq!(compile(&registry(), &hero(), &context).unwrap_err().code(), "missing_context");
}

#[test]
fn tampered_template_fails_to_load() {
    let source = templates_root().join("rgb-payment-v1");
    let tmp = std::env::temp_dir().join(format!("poweredge-tamper-{}", std::process::id()));
    let dir = tmp.join("rgb-payment-v1");
    fs::create_dir_all(&dir).unwrap();
    fs::copy(source.join("manifest.json"), dir.join("manifest.json")).unwrap();
    let graph = fs::read_to_string(source.join("graph.json")).unwrap();
    fs::write(dir.join("graph.json"), graph.replace("\"timeoutSeconds\": 900", "\"timeoutSeconds\": 9")).unwrap();
    let err = TemplateRegistry::load_dir(&tmp).unwrap_err();
    fs::remove_dir_all(&tmp).ok();
    assert_eq!(err.code(), "template_integrity", "{err}");
}

#[test]
fn validator_rejects_bypass_injection_secrets_and_broken_wires() {
    let registry = registry();
    let manifest = &registry.resolve("rgb-payment-v1", "1.0.0").unwrap().manifest;
    let flow = compile(&registry, &hero(), &ctx()).unwrap();
    let id_of = |t: &str| node(&flow.nodes, t)["id"].as_str().unwrap().to_owned();
    assert!(validate(manifest, &flow.nodes, Environment::Mutinynet).is_ok());

    // 1. Rewire plan straight to send: approval bypass.
    let mut bypass = flow.nodes.clone();
    for n in &mut bypass {
        if n["type"] == "rgb-plan" {
            n["wires"] = json!([[id_of("rgb-send-once")], [id_of("http response")]]);
        }
    }
    // 2. Remove the approval gate entirely.
    let no_gate: Vec<Value> = flow
        .nodes
        .iter()
        .filter(|n| n["type"] != "approval-gate")
        .cloned()
        .map(|mut n| {
            if n["type"] == "rgb-plan" {
                n["wires"] = json!([[], []]);
            }
            n
        })
        .collect();
    // 3. Inject an exec node.
    let mut exec = flow.nodes.clone();
    exec.push(json!({"id": "ffffffffffffffff", "type": "exec", "z": WORKSPACE, "command": "sh", "wires": [[]]}));
    // 4. Secret in a node.
    let mut secret = flow.nodes.clone();
    secret[0]["apiKey"] = json!("sk-should-not-be-here");
    // 5. Wire to a node that does not exist.
    let mut dangling = flow.nodes.clone();
    for n in &mut dangling {
        if n["type"] == "rgb-receipt" {
            n["wires"] = json!([["0123456789abcdef"]]);
        }
    }
    // 6. Unknown node type allowed nowhere.
    let mut unknown = flow.nodes.clone();
    unknown.push(json!({"id": "eeeeeeeeeeeeeeee", "type": "mqtt out", "z": WORKSPACE, "wires": []}));

    for (name, nodes) in
        [("bypass", bypass), ("exec", exec), ("secret", secret), ("dangling", dangling), ("unknown", unknown)]
    {
        let err = validate(manifest, &nodes, Environment::Mutinynet).unwrap_err();
        assert_eq!(err.code(), "unsafe_graph", "{name}: {err}");
    }
    // The gateless graph still contains a spending node, so it must fail too.
    assert!(validate(manifest, &no_gate, Environment::Mutinynet).is_err());
}
