//! `poweredge-template digest <template-dir>`   print the digest to pin in manifest.json
//! `poweredge-template verify <templates-root>`  load every template and verify digests
//! `poweredge-template compile <templates-root> <intent.json> <context.json>`
//!     compile + validate; print `{"flow": {...metadata}, "nodes": [tab, ...nodes]}`
//!     context.json: {"workspace_id": "...", "values": {...}, "assets": {"TUSDT": {"precision": 6}}}

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use poweredge::registry::compute_dir_digest;
use poweredge::{compile, AssetInfo, BindContext, Error, TemplateRegistry, WorkflowIntent};
use serde_json::{json, Value};

fn fail(err: &Error) -> ExitCode {
    eprintln!("error [{}]: {err}", err.code());
    ExitCode::FAILURE
}

fn read(path: &str) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|err| Error::Io(format!("{path}: {err}")))
}

fn context(raw: &Value) -> Result<BindContext, Error> {
    let bad = |what: &str| Error::Context(what.to_owned());
    let workspace_id = raw["workspace_id"].as_str().ok_or_else(|| bad("workspace_id"))?.to_owned();
    let mut values = BTreeMap::new();
    for (k, v) in raw["values"].as_object().ok_or_else(|| bad("values"))? {
        values.insert(k.clone(), v.as_str().ok_or_else(|| bad(k))?.to_owned());
    }
    let mut assets = BTreeMap::new();
    for (ticker, info) in raw["assets"].as_object().ok_or_else(|| bad("assets"))? {
        let precision = info["precision"].as_u64().filter(|p| *p <= 18).ok_or_else(|| bad(ticker))?;
        assets.insert(ticker.to_ascii_uppercase(), AssetInfo { precision: precision as u8 });
    }
    Ok(BindContext { workspace_id, values, assets })
}

fn run_compile(root: &str, intent_path: &str, ctx_path: &str) -> Result<Value, Error> {
    let registry = TemplateRegistry::load_dir(Path::new(root))?;
    let intent = WorkflowIntent::parse(&read(intent_path)?)?;
    let raw: Value = serde_json::from_slice(&read(ctx_path)?).map_err(|e| Error::Context(e.to_string()))?;
    let ctx = context(&raw)?;
    let flow = compile(&registry, &intent, &ctx)?;
    let mut nodes = vec![json!({"id": ctx.workspace_id, "type": "tab", "label": "PowerEdge payouts"})];
    nodes.extend(flow.nodes);
    Ok(json!({
        "flow": {
            "template_id": flow.template_id, "template_version": flow.template_version,
            "template_digest": flow.template_digest, "intent_digest": flow.intent_digest,
            "flow_digest": flow.flow_digest, "bound": flow.bound,
        },
        "nodes": nodes,
    }))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    match (arg(1), arg(2), arg(3), arg(4)) {
        (Some("digest"), Some(dir), None, None) => match compute_dir_digest(Path::new(dir)) {
            Ok(digest) => {
                println!("{digest}");
                ExitCode::SUCCESS
            }
            Err(err) => fail(&err),
        },
        (Some("verify"), Some(root), None, None) => match TemplateRegistry::load_dir(Path::new(root)) {
            Ok(registry) => {
                for t in registry.iter() {
                    println!("ok {}@{} {}", t.manifest.id, t.manifest.version, t.digest());
                }
                ExitCode::SUCCESS
            }
            Err(err) => fail(&err),
        },
        (Some("compile"), Some(root), Some(intent), Some(ctx)) => match run_compile(root, intent, ctx) {
            Ok(out) => {
                println!("{}", serde_json::to_string_pretty(&out).expect("json"));
                ExitCode::SUCCESS
            }
            Err(err) => fail(&err),
        },
        _ => {
            eprintln!(
                "usage: poweredge-template digest <template-dir> | verify <templates-root> | \
                 compile <templates-root> <intent.json> <context.json>"
            );
            ExitCode::from(2)
        }
    }
}
