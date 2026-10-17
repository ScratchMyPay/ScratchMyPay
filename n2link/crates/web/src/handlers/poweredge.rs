//! RGB2link PowerEdge: guided payout app + local-QVAC template drafting (`nodes_rgb`).
//!
//! Describe -> `POST /poweredge-app/api/draft`: local QVAC turns plain words into a
//!   `WorkflowIntent` (JSON-schema constrained); the pinned template compiler and validator
//!   decide. The AI never sees invoices, wallets or keys.
//! Review   -> the plain-language card is built from compiled values, not from AI text.
//! Set up   -> `POST /poweredge-app/api/setup`: the server (not the browser) deploys the
//!   compiled flow through n2link's normal deploy path.
//! Approve / Track -> the app talks only to the deployed flow's `/poweredge/*` endpoints.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::{Extension, Json};
use n2link_core::runtime::egress::EgressPurpose;
use poweredge::{
    AssetInfo, BindContext, CompiledFlow, Environment, SlotKind, TemplateRegistry, WorkflowIntent, compile,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::WebState;
use super::deploy;
use super::reply::api_error;

const APP_HTML: &str = include_str!("../../poweredge-app/index.html");
/// Fixed editor tab that holds the active PowerEdge payout flow.
pub const POWEREDGE_TAB: &str = "9e0d9e0d9e0d9e0d";
/// Payout-path node types: only the PowerEdge app may place them, and only on its managed tab.
/// The Flow Copilot never sees or proposes them.
pub const RESERVED_TYPES: &[&str] =
    &["rgb-plan", "approval-gate", "rgb-approve", "rgb-send-once", "rgb-refresh", "rgb-receipt"];

/// Which wallet a node type should use when a flow has both (Copilot drafts).
pub fn preferred_wallet_role(node_type: &str) -> &'static str {
    if matches!(node_type, "rgb-invoice") { "recipient" } else { "payer" }
}

/// Fixed id of the "QVAC (local)" `ai-provider` config node (also used by Flow Copilot).
pub const QVAC_PROVIDER: &str = "9e0d9e0d9e0d0a1c";
/// Fixed id of the "Treasury (Tether WDK)" `wdk-wallet-ref` config node.
pub const WDK_TREASURY: &str = "9e0d9e0d9e0d0dd1";
const MAX_PROMPT_CHARS: usize = 1_000;
const DRAFT_TTL: Duration = Duration::from_secs(30 * 60);
const QVAC_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_REPLY_BYTES: usize = 64 * 1024;

struct Settings {
    templates_dir: PathBuf,
    qvac_url: String,
    qvac_key: String,
    qvac_model: String,
    /// What people see for the model (the alias stays the API name): e.g. "Qwen3-8B".
    qvac_model_label: String,
    payer_url: String,
    recipient_url: String,
    wdk_url: String,
    wallet_token: String,
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default.to_owned())
}

fn read_secret(var: &str) -> Result<String, String> {
    let path = std::env::var(var).map_err(|_| format!("{var} is not set"))?;
    let value = std::fs::read_to_string(&path).map_err(|_| format!("{var} file is not readable"))?;
    Ok(value.trim().to_owned())
}

/// The AI and the wallets must be on this computer: loopback only, never a cloud fallback.
fn loopback(url: &str, what: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{what} URL is invalid"))?;
    if parsed.scheme() != "http" || !matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")) {
        return Err(format!("{what} must be on this computer (http://127.0.0.1:<port>)"));
    }
    Ok(url.trim_end_matches('/').to_owned())
}

fn settings() -> Result<Settings, String> {
    Ok(Settings {
        templates_dir: PathBuf::from(
            std::env::var("POWEREDGE_TEMPLATES_DIR").map_err(|_| "POWEREDGE_TEMPLATES_DIR is not set")?,
        ),
        qvac_url: loopback(&env_or("POWEREDGE_QVAC_URL", "http://127.0.0.1:11435/v1"), "QVAC")?,
        qvac_key: read_secret("POWEREDGE_QVAC_KEY_FILE")?,
        qvac_model: env_or("POWEREDGE_QVAC_MODEL", "poweredge-intent"),
        qvac_model_label: env_or("POWEREDGE_QVAC_MODEL_LABEL", "Qwen3-8B"),
        payer_url: loopback(&env_or("POWEREDGE_PAYER_URL", "http://127.0.0.1:8711"), "payer wallet")?,
        recipient_url: loopback(&env_or("POWEREDGE_RECIPIENT_URL", "http://127.0.0.1:8712"), "recipient wallet")?,
        wdk_url: loopback(&env_or("POWEREDGE_WDK_URL", "http://127.0.0.1:8713"), "WDK treasury")?,
        wallet_token: read_secret("POWEREDGE_WALLET_TOKEN_FILE")?,
    })
}

struct Draft {
    created: Instant,
    compiled: CompiledFlow,
    provenance: String,
}

/// Model text must never look like a template placeholder.
fn defang(text: &str) -> String {
    text.replace("{{", "{ {").replace("}}", "} }")
}

/// Human-readable record of how this flow was made, stored in the flow itself.
fn provenance_text(
    prompt: &str,
    intent: &WorkflowIntent,
    model: &str,
    latency_ms: u128,
    template: &poweredge::Template,
) -> String {
    let params = serde_json::to_string(&intent.parameters).unwrap_or_default();
    let m = &template.manifest;
    defang(&format!(
        "Drafted by LOCAL AI: QVAC, model `{model}`, on this computer in {:.1} s. The AI only proposed this payout; it cannot approve or send.\n\n\
Organizer's words: \"{}\"\n\n\
AI structured request (WorkflowIntent v1): template `{}` v{}, environment `{}`, parameters {params}\n\n\
AI explanation (not authoritative): {}\n\n\
Decided by code, not by the AI: pinned template `{}`@{} digest {}; amounts converted with the wallet's reported precision; \
validator checks allowlisted nodes, an approval gate before the only spending node, and loopback-only endpoints. \
A person approves the exact plan in the ScratchMyPay app; the wallet journal sends at most once.",
        latency_ms as f64 / 1000.0,
        prompt.chars().take(300).collect::<String>(),
        intent.template_id,
        intent.template_version,
        intent.environment.as_str(),
        intent.rationale.as_deref().unwrap_or("-"),
        m.id,
        m.version,
        m.digest,
    ))
}

fn drafts() -> &'static Mutex<HashMap<String, Draft>> {
    static DRAFTS: OnceLock<Mutex<HashMap<String, Draft>>> = OnceLock::new();
    DRAFTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fail(status: StatusCode, code: &str, message: &str) -> Response {
    api_error(status, code, message)
}

#[allow(clippy::result_large_err)] // axum handlers return the ready-made error Response
async fn http_client(state: &WebState, purpose: EgressPurpose, url: &str) -> Result<reqwest::Client, Response> {
    let engine = state.engine.read().await.clone();
    let policy = match engine {
        Some(engine) => engine.egress_policy().snapshot(),
        None => {
            return Err(fail(StatusCode::SERVICE_UNAVAILABLE, "runtime_unavailable", "the flow engine is not running"));
        }
    };
    policy
        .http_client(purpose, url)
        .await
        .map_err(|err| fail(StatusCode::FORBIDDEN, "egress_denied", &format!("network policy refused {url}: {err}")))
}

/// Assets the payer wallet holds: ticker -> (precision, name, spendable). Authoritative units.
#[allow(clippy::result_large_err)] // axum handlers return the ready-made error Response
async fn payer_assets(state: &WebState, s: &Settings) -> Result<BTreeMap<String, Value>, Response> {
    let url = format!("{}/assets", s.payer_url);
    let client = http_client(state, EgressPurpose::HttpNode, &url).await?;
    let unreachable =
        |_| fail(StatusCode::SERVICE_UNAVAILABLE, "wallet_unreachable", "the organizer wallet service is not running");
    let response = client
        .get(&url)
        .bearer_auth(&s.wallet_token)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(unreachable)?;
    let body: Value = response
        .json()
        .await
        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "wallet_error", "wallet returned an unreadable answer"))?;
    let mut out = BTreeMap::new();
    for asset in body["assets"].as_array().cloned().unwrap_or_default() {
        if let Some(ticker) = asset["ticker"].as_str() {
            out.insert(ticker.to_ascii_uppercase(), asset.clone());
        }
    }
    Ok(out)
}

/// Network fee for one payout from the Tether WDK treasury service; `null` if it is not running.
async fn wdk_fee(state: &WebState, s: &Settings) -> Value {
    let url = format!("{}/fees?count=1", s.wdk_url);
    let Ok(client) = http_client(state, EgressPurpose::HttpNode, &url).await else {
        return Value::Null;
    };
    let reply = client.get(&url).bearer_auth(&s.wallet_token).timeout(Duration::from_secs(8)).send().await;
    match reply {
        Ok(r) if r.status().is_success() => {
            let body: Value = r.json().await.unwrap_or(Value::Null);
            let row = &body["bySpeed"]["normal"];
            if row.is_null() {
                return Value::Null;
            }
            json!({"engine": "Tether WDK", "feeRateSatVb": row["feeRateSatVb"], "feeSat": row["feeSat"],
                   "witnessSat": body["witnessSat"], "perPayoutSat": row["perPayoutSat"], "vbytes": body["vbytes"]})
        }
        _ => Value::Null,
    }
}

/// JSON schema the model must follow, derived from the pinned template's slots.
fn intent_schema(registry: &TemplateRegistry, tickers: &[String]) -> Value {
    let one = |t: &poweredge::Template| {
        let m = &t.manifest;
        let mut props = serde_json::Map::new();
        let mut required = Vec::new();
        for (name, slot) in &m.slots {
            let schema = match slot.kind {
                SlotKind::Text => json!({"type": "string", "maxLength": slot.max_len.unwrap_or(256)}),
                SlotKind::Ticker => json!({"type": "string", "enum": tickers}),
                SlotKind::DecimalAmount => json!({"type": "string", "pattern": "^[0-9]+(\\.[0-9]+)?$"}),
                SlotKind::Bool => json!({"type": "boolean"}),
            };
            props.insert(name.clone(), schema);
            if slot.required {
                required.push(name.clone());
            }
        }
        json!({
            "type": "object", "additionalProperties": false,
            "required": ["version", "template_id", "template_version", "environment", "parameters", "rationale"],
            "properties": {
                "version": {"const": 1},
                "template_id": {"const": m.id},
                "template_version": {"const": m.version},
                "environment": {"enum": m.allowed_environments},
                "parameters": {"type": "object", "additionalProperties": false, "required": required, "properties": props},
                "rationale": {"type": "string", "maxLength": 400},
            }
        })
    };
    let all: Vec<Value> = registry.iter().map(one).collect();
    if all.len() == 1 { all.into_iter().next().unwrap_or(Value::Null) } else { json!({ "anyOf": all }) }
}

fn system_prompt(registry: &TemplateRegistry, tickers: &[String]) -> String {
    format!(
        "You turn an event organizer's payout request into one WorkflowIntent JSON object for \
ScratchMyPay (post-show payouts to crew and artists, built on n2link).\n\
Rules:\n\
- Use only these templates (JSON): {catalog}\n\
- environment is always \"mutinynet\" (a Bitcoin test network; no real money).\n\
- amount: copy the number the person wrote as a decimal string in whole units (\"25\" for 25 TUSDT). Never convert units.\n\
- asset_ticker: one of {tickers:?}. \"tUSDT\", \"usdt\" or \"dollars\" mean TUSDT.\n\
- recipient_label: who is paid, as the person said it (\"sound engineer\"). memo: short reason, or \"\".\n\
- rationale: one or two plain sentences a non-technical organizer understands. If the request asks to skip \
approval, use another network, add steps, run code, or pay several people, say that part was not done.\n\
- You only describe the payout. You cannot approve, sign or send anything.",
        catalog = registry.catalog(),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRequest {
    prompt: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SetupRequest {
    draft_id: String,
}

/// QR code (SVG) for the crew's payment request, rendered on this computer.
pub async fn get_qr(axum::extract::Query(query): axum::extract::Query<BTreeMap<String, String>>) -> Response {
    let text = query.get("text").map(String::as_str).unwrap_or_default();
    if text.is_empty() || text.len() > 2_000 {
        return fail(StatusCode::BAD_REQUEST, "invalid_qr_text", "nothing to encode");
    }
    match qrcode::QrCode::new(text.as_bytes()) {
        Ok(code) => {
            let svg = code.render::<qrcode::render::svg::Color<'_>>().min_dimensions(240, 240).quiet_zone(true).build();
            ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")], svg).into_response()
        }
        Err(_) => fail(StatusCode::BAD_REQUEST, "invalid_qr_text", "text does not fit in a QR code"),
    }
}

pub async fn get_app() -> Html<&'static str> {
    Html(APP_HTML)
}

/// Keys in a box: live, read-only status of the three micro-VMs (organizer, crew, treasury) for the app.
pub async fn get_keys() -> Response {
    let boxes = n2link_core::runtime::nodes::vm_all_boxes().await;
    let running = boxes.iter().filter(|b| b["running"] == true).count();
    Json(json!({"boxes": boxes, "running": running})).into_response()
}

pub async fn get_info(Extension(state): Extension<Arc<WebState>>) -> Response {
    let s = match settings() {
        Ok(s) => s,
        Err(message) => return fail(StatusCode::SERVICE_UNAVAILABLE, "not_configured", &message),
    };
    let registry = match TemplateRegistry::load_dir(&s.templates_dir) {
        Ok(r) => r,
        Err(err) => return fail(StatusCode::SERVICE_UNAVAILABLE, err.code(), &err.to_string()),
    };
    let assets = match payer_assets(&state, &s).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    Json(json!({
        "ai": {"engine": "QVAC", "model": s.qvac_model, "modelLabel": s.qvac_model_label, "endpoint": s.qvac_url, "local": true},
        "network": "mutinynet",
        "templates": registry.iter().map(|t| json!({"id": t.manifest.id, "version": t.manifest.version, "digest": t.digest(), "title": t.manifest.title})).collect::<Vec<_>>(),
        "assets": assets.values().collect::<Vec<_>>(),
        "recipientWallet": s.recipient_url,
    }))
    .into_response()
}

pub async fn post_draft(
    Extension(state): Extension<Arc<WebState>>,
    headers: HeaderMap,
    Json(request): Json<DraftRequest>,
) -> Response {
    let actor = state.auth.actor_from_headers(&headers);
    let prompt = request.prompt.trim().to_owned();
    if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return fail(StatusCode::BAD_REQUEST, "invalid_prompt", "describe the payout in up to 1000 characters");
    }
    let s = match settings() {
        Ok(s) => s,
        Err(message) => return fail(StatusCode::SERVICE_UNAVAILABLE, "not_configured", &message),
    };
    let registry = match TemplateRegistry::load_dir(&s.templates_dir) {
        Ok(r) => r,
        Err(err) => return fail(StatusCode::SERVICE_UNAVAILABLE, err.code(), &err.to_string()),
    };
    let assets = match payer_assets(&state, &s).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };
    if assets.is_empty() {
        return fail(StatusCode::CONFLICT, "no_assets", "the organizer wallet holds no RGB asset to pay with");
    }
    let tickers: Vec<String> = assets.keys().cloned().collect();

    // --- local QVAC: words -> constrained intent (untrusted) ---
    let url = format!("{}/chat/completions", s.qvac_url);
    let client = match http_client(&state, EgressPurpose::AiProvider, &url).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let body = json!({
        "model": s.qvac_model,
        "temperature": 0,
        "max_tokens": 400,
        "messages": [
            {"role": "system", "content": system_prompt(&registry, &tickers)},
            {"role": "user", "content": prompt},
        ],
        "response_format": {"type": "json_schema", "json_schema": {"name": "workflow_intent", "strict": true, "schema": intent_schema(&registry, &tickers)}},
    });
    let started = Instant::now();
    let reply = client.post(&url).bearer_auth(&s.qvac_key).timeout(QVAC_TIMEOUT).json(&body).send().await;
    let reply = match reply {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => return fail(StatusCode::BAD_GATEWAY, "ai_error", &format!("local AI answered HTTP {}", r.status())),
        Err(_) => {
            return fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "ai_unreachable",
                "the local AI (QVAC) is not running on this computer",
            );
        }
    };
    let latency_ms = started.elapsed().as_millis();
    let bytes = match reply.bytes().await {
        Ok(b) if b.len() <= MAX_REPLY_BYTES => b,
        _ => return fail(StatusCode::BAD_GATEWAY, "ai_error", "local AI reply was unreadable or too large"),
    };
    let reply: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let content = reply.pointer("/choices/0/message/content").and_then(Value::as_str).unwrap_or_default().to_owned();
    let model = reply["model"].as_str().unwrap_or(&s.qvac_model).to_owned();

    let intent = match WorkflowIntent::parse(content.as_bytes()) {
        Ok(i) => i,
        Err(err) => {
            state.history.record_copilot_rejected(&actor.username, "poweredge_ai_not_understood");
            return Json(json!({"ok": false, "error": {"code": "ai_not_understood", "message": err.to_string()}, "ai": {"raw": content, "model": model, "latencyMs": latency_ms}}))
                .into_response();
        }
    };

    // --- deterministic: pinned template, typed binding, validation ---
    let provenance = match registry.resolve(&intent.template_id, &intent.template_version) {
        Ok(template) => provenance_text(&prompt, &intent, &model, latency_ms, template),
        Err(err) => {
            return Json(
                json!({"ok": false, "error": {"code": err.code(), "message": err.to_string()}, "intent": intent}),
            )
            .into_response();
        }
    };
    let ctx = BindContext {
        workspace_id: POWEREDGE_TAB.into(),
        values: BTreeMap::from([
            ("payer_wallet_url".into(), s.payer_url.clone()),
            ("recipient_wallet_url".into(), s.recipient_url.clone()),
            ("provenance".into(), provenance.clone()),
        ]),
        assets: assets
            .iter()
            .filter_map(|(t, a)| a["precision"].as_u64().map(|p| (t.clone(), AssetInfo { precision: p.min(18) as u8 })))
            .collect(),
    };
    let rationale = intent.rationale.clone().unwrap_or_default();
    let compiled = match compile(&registry, &intent, &ctx) {
        Ok(c) => c,
        Err(err) => {
            state.history.record_copilot_rejected(&actor.username, err.code());
            return Json(json!({"ok": false, "error": {"code": err.code(), "message": err.to_string()}, "intent": intent, "ai": {"model": model, "latencyMs": latency_ms, "explanation": rationale}}))
                .into_response();
        }
    };

    let bound = |k: &str| compiled.bound.get(k).cloned().unwrap_or(Value::Null);
    let ticker = bound("asset_ticker").as_str().unwrap_or_default().to_owned();
    let asset = assets.get(&ticker).cloned().unwrap_or(Value::Null);
    let network_fee = wdk_fee(&state, &s).await;
    let draft_id =
        poweredge::canonical::sha256_hex(format!("{}:{:?}", compiled.flow_digest, Instant::now()).as_bytes())[..24]
            .to_owned();
    let response = json!({
        "ok": true,
        "draftId": draft_id,
        "review": {
            "recipient": bound("recipient_label"), "amount": bound("amount"), "amountBaseUnits": bound("amount_base_units"),
            "assetTicker": ticker, "assetName": asset["name"], "assetId": asset["asset_id"], "memo": bound("memo"),
            "network": "mutinynet",
            "networkFee": network_fee,
        },
        "ai": {"engine": "QVAC", "model": model, "modelLabel": s.qvac_model_label, "latencyMs": latency_ms, "local": true, "explanation": rationale},
        "template": {"id": compiled.template_id, "version": compiled.template_version, "digest": compiled.template_digest},
        "flow": {"digest": compiled.flow_digest, "intentDigest": compiled.intent_digest, "nodes": compiled.nodes.len()},
        "intent": intent,
    });
    {
        let mut store = drafts().lock().unwrap_or_else(|p| p.into_inner());
        store.retain(|_, d| d.created.elapsed() < DRAFT_TTL);
        store.insert(draft_id, Draft { created: Instant::now(), compiled, provenance });
    }
    state.history.record_copilot_produced(&actor.username, 1);
    let _ = state.audit.record(&actor.username, "poweredge.draft", None).await;
    Json(response).into_response()
}

pub async fn post_setup(
    Extension(state): Extension<Arc<WebState>>,
    headers: HeaderMap,
    Json(request): Json<SetupRequest>,
) -> Response {
    let actor = state.auth.actor_from_headers(&headers);
    let (nodes, provenance) = {
        let store = drafts().lock().unwrap_or_else(|p| p.into_inner());
        match store.get(&request.draft_id) {
            Some(d) if d.created.elapsed() < DRAFT_TTL => (d.compiled.nodes.clone(), d.provenance.clone()),
            _ => return fail(StatusCode::NOT_FOUND, "draft_expired", "this payout draft expired; describe it again"),
        }
    };
    let s = match settings() {
        Ok(s) => s,
        Err(message) => return fail(StatusCode::SERVICE_UNAVAILABLE, "not_configured", &message),
    };
    let Some(path) = state.flows_file_path.read().await.clone() else {
        return fail(StatusCode::SERVICE_UNAVAILABLE, "runtime_unavailable", "flows file is not configured");
    };
    let _deploy = state.deploy.lock().await;
    let current = match deploy::load_flows_array(&path).await {
        Ok(flows) => flows,
        Err(err) => return fail(StatusCode::INTERNAL_SERVER_ERROR, "flows_unreadable", &err),
    };
    // Replace the previous PowerEdge payout (one active payout flow); keep everything else.
    let mut flows: Vec<Value> = current
        .into_iter()
        .filter(|n| {
            n["id"] != POWEREDGE_TAB
                && n["z"] != POWEREDGE_TAB
                && n["type"] != "rgb-wallet-ref"
                && n["id"] != QVAC_PROVIDER
                && n["id"] != WDK_TREASURY
        })
        .collect();
    flows.push(json!({
        "id": POWEREDGE_TAB, "type": "tab", "label": "ScratchMyPay payouts",
        "info": format!("{provenance}\n\nManaged by the ScratchMyPay app. Editing this tab changes the flow revision and voids pending approvals."),
    }));
    // The same local model, visible to n2link: Configuration nodes + Flow Copilot.
    // Provider kind `cortex` = n2link's generic OpenAI Chat Completions client (Bearer auth),
    // which QVAC serves correctly; QVAC's /v1/responses ignores message content (tested 2026-10-15).
    // The key goes to n2link's encrypted credential sidecar, never into flows.json.
    flows.push(json!({
        "id": QVAC_PROVIDER, "type": "ai-provider", "name": "QVAC (local)",
        "provider": "cortex", "baseUrl": s.qvac_url, "defaultModel": s.qvac_model, "organization": "",
        "credentials": {"apiKey": s.qvac_key},
    }));
    // Tether WDK treasury (read-only): balance, fees, payout cost for flows and the Copilot.
    flows.push(
        json!({"id": WDK_TREASURY, "type": "wdk-wallet-ref", "name": "Treasury (Tether WDK)", "baseUrl": s.wdk_url}),
    );
    flows.extend(nodes);
    match deploy::commit(&state, &path, flows).await {
        Ok((_, revision)) => {
            state.comms.send_deploy_notification(true, Some(&revision)).await;
            state.history.record_deploy_accepted(&actor.username, &revision, "poweredge", 1);
            let _ = state.audit.record(&actor.username, "poweredge.setup", Some(&revision)).await;
            Json(json!({"ok": true, "revision": revision, "tab": POWEREDGE_TAB})).into_response()
        }
        Err(err) => {
            log::warn!("PowerEdge setup deploy failed");
            err.into_response()
        }
    }
}

/// The deployed payout flow's loopback endpoints (fixed by the pinned template).
fn flow_endpoint(name: &str) -> Option<(reqwest::Method, String)> {
    let (method, port, path) = match name {
        "invoices" => (reqwest::Method::POST, 1891, "/poweredge/invoices"),
        "payouts" => (reqwest::Method::POST, 1892, "/poweredge/payouts"),
        "status" => (reqwest::Method::GET, 1893, "/poweredge/payouts/status"),
        "approvals" => (reqwest::Method::POST, 1894, "/poweredge/approvals"),
        _ => return None,
    };
    Some((method, format!("http://127.0.0.1:{port}{path}")))
}

/// Same-origin bridge from the app to the payout flow. Only the four fixed endpoints.
pub async fn flow_proxy(
    Extension(state): Extension<Arc<WebState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<BTreeMap<String, String>>,
    body: Option<Json<Value>>,
) -> Response {
    let Some((method, url)) = flow_endpoint(&name) else {
        return fail(StatusCode::NOT_FOUND, "unknown_endpoint", "no such ScratchMyPay step");
    };
    let client = match http_client(&state, EgressPurpose::HttpNode, &url).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let request = if method == reqwest::Method::GET {
        client.get(&url).query(&query)
    } else {
        client.post(&url).json(&body.map(|Json(v)| v).unwrap_or_else(|| json!({})))
    };
    let response = match request.timeout(Duration::from_secs(300)).send().await {
        Ok(r) => r,
        Err(_) => {
            return fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "flow_not_ready",
                "the payout flow is not running; set up the payout first",
            );
        }
    };
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let bytes = response.bytes().await.unwrap_or_default();
    let value: Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({"ok": false, "error": {"code": "flow_error", "message": String::from_utf8_lossy(&bytes).chars().take(300).collect::<String>()}}));
    (status, Json(value)).into_response()
}

/// Deploy guard: whoever deploys (app, editor, API), a "PowerEdge payouts" tab must still pass
/// the PowerEdge validator against the template it was compiled from. Other tabs are untouched.
pub fn guard_managed_tab(flows: &[Value]) -> Result<(), String> {
    if let Some(node) = flows
        .iter()
        .find(|n| n["z"] != POWEREDGE_TAB && n["type"].as_str().is_some_and(|t| RESERVED_TYPES.contains(&t)))
    {
        return Err(format!(
            "\"{}\" nodes only run inside the ScratchMyPay payouts tab, where every payout has an approval step. \
Prepare payouts in the ScratchMyPay app; payment requests and QR codes are fine on any tab.",
            node["type"].as_str().unwrap_or_default()
        ));
    }
    let tab: Vec<Value> = flows.iter().filter(|n| n["z"] == POWEREDGE_TAB).cloned().collect();
    if tab.is_empty() {
        return Ok(());
    }
    let managed = "The \"ScratchMyPay payouts\" tab is managed by the ScratchMyPay app";
    let digest =
        tab.iter().find(|n| n["type"] == "rgb-plan").and_then(|n| n["templateDigest"].as_str()).ok_or_else(|| {
            format!("{managed}: its payout plan node is missing; prepare the payout again in the app")
        })?;
    let dir = std::env::var("POWEREDGE_TEMPLATES_DIR")
        .map_err(|_| format!("{managed}: POWEREDGE_TEMPLATES_DIR is not set, so it cannot be checked"))?;
    let registry = TemplateRegistry::load_dir(std::path::Path::new(&dir)).map_err(|e| format!("{managed}: {e}"))?;
    let template = registry
        .iter()
        .find(|t| t.digest() == digest)
        .ok_or_else(|| format!("{managed}: it was built from a template that is not pinned here"))?;
    let refs: Vec<Value> = flows.iter().filter(|n| n["type"] == "rgb-wallet-ref").cloned().collect();
    let environment: Environment = refs
        .first()
        .and_then(|n| serde_json::from_value(n["environment"].clone()).ok())
        .ok_or_else(|| format!("{managed}: its wallet nodes must name a test network"))?;
    let mut nodes = refs;
    nodes.extend(tab);
    poweredge::validate(&template.manifest, &nodes, environment).map_err(|e| {
        format!("{managed} and this change is not allowed ({e}). Put other nodes on another tab, or prepare the payout again in the app.")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled_tab() -> Vec<Value> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../templates");
        // SAFETY: test-only; every test in this module sets the same value.
        unsafe { std::env::set_var("POWEREDGE_TEMPLATES_DIR", &root) };
        let registry = TemplateRegistry::load_dir(&root).unwrap();
        let intent = WorkflowIntent::parse(
            br#"{"version":1,"template_id":"rgb-payment-v1","template_version":"1.0.0","environment":"mutinynet",
                "parameters":{"recipient_label":"sound engineer","amount":"25","asset_ticker":"TUSDT","memo":"show"}}"#,
        )
        .unwrap();
        let ctx = BindContext {
            workspace_id: POWEREDGE_TAB.into(),
            values: BTreeMap::from([
                ("payer_wallet_url".into(), "http://127.0.0.1:8711".into()),
                ("recipient_wallet_url".into(), "http://127.0.0.1:8712".into()),
                ("provenance".into(), "test".into()),
            ]),
            assets: BTreeMap::from([("TUSDT".into(), AssetInfo { precision: 6 })]),
        };
        let mut flows =
            vec![json!({"id": POWEREDGE_TAB, "type": "tab"}), json!({"id": "1111111111111111", "type": "tab"})];
        flows.extend(compile(&registry, &intent, &ctx).unwrap().nodes);
        flows
    }

    #[test]
    fn guard_accepts_the_compiled_tab_and_ignores_other_tabs() {
        let mut flows = compiled_tab();
        flows.push(json!({"id": "2222222222222222", "type": "inject", "z": "1111111111111111", "wires": [[]]}));
        assert!(guard_managed_tab(&flows).is_ok());
        assert!(guard_managed_tab(&[json!({"id": "1", "type": "tab"})]).is_ok());
    }

    #[test]
    fn spending_nodes_are_refused_outside_the_managed_tab() {
        let mut flows = compiled_tab();
        flows.push(json!({"id": "4444444444444444", "type": "rgb-send-once", "z": "1111111111111111", "wallet": "x", "wires": [[], []]}));
        assert!(guard_managed_tab(&flows).unwrap_err().contains("only run inside the ScratchMyPay payouts tab"));
        let mut ok = compiled_tab();
        ok.push(json!({"id": "5555555555555555", "type": "qr-code", "z": "1111111111111111", "wires": [[]]}));
        assert!(guard_managed_tab(&ok).is_ok());
    }

    #[test]
    fn guard_refuses_edits_that_break_the_payout_rules() {
        // The stock editor dropped host/port: endpoint would move to 0.0.0.0.
        let mut dropped = compiled_tab();
        for n in &mut dropped {
            if n["type"] == "http in" {
                n.as_object_mut().unwrap().remove("host");
            }
        }
        let err = guard_managed_tab(&dropped).unwrap_err();
        assert!(err.contains("127.0.0.1"), "{err}");

        // Someone dropped a debug node (or a Copilot draft) onto the managed tab.
        let mut extra = compiled_tab();
        extra.push(json!({"id": "3333333333333333", "type": "debug", "z": POWEREDGE_TAB, "wires": []}));
        assert!(guard_managed_tab(&extra).unwrap_err().contains("another tab"));
    }
}

// ───────────────────────────── Pay-to-enter door ──────────────────────────────
//
// An organizer buys a 24 h pass by paying `POWEREDGE_DOOR_PRICE_SAT` test sats to a fresh
// address of the Tether WDK treasury (services/wdk-btc owns the door state). Once the WDK
// service reports the attempt paid, n2link issues the same kind of session token the
// password login issues, with the deployer role, so the editor and the app open unchanged.
// Tokens are issued once per attempt and remembered here so polling cannot mint more.

const LOGIN_HTML: &str = include_str!("../../poweredge-app/login.html");
const PASS_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn door_tokens() -> &'static Mutex<HashMap<u64, String>> {
    static TOKENS: OnceLock<Mutex<HashMap<u64, String>>> = OnceLock::new();
    TOKENS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn get_login() -> Html<&'static str> {
    Html(LOGIN_HTML)
}

#[allow(clippy::result_large_err)] // axum handlers return the ready-made error Response
async fn wdk_call(
    state: &WebState,
    s: &Settings,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, Response> {
    let url = format!("{}{}", s.wdk_url, path);
    let client = http_client(state, EgressPurpose::HttpNode, &url).await?;
    let mut req = client.request(method, &url).bearer_auth(&s.wallet_token).timeout(Duration::from_secs(30));
    if let Some(body) = body {
        req = req.json(&body);
    }
    let reply = req
        .send()
        .await
        .map_err(|e| fail(StatusCode::BAD_GATEWAY, "door_unavailable", &format!("WDK treasury not reachable: {e}")))?;
    let status = reply.status();
    let value: Value = reply.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let code = value["error"]["code"].as_str().unwrap_or("door_error").to_owned();
        let message = value["error"]["message"].as_str().unwrap_or("the door refused").to_owned();
        return Err(fail(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY), &code, &message));
    }
    Ok(value)
}

#[derive(Debug, Deserialize)]
pub struct DoorStart {
    #[serde(default)]
    label: String,
}

/// Start an attempt: a fresh treasury address for one pass.
pub async fn door_start(Extension(state): Extension<Arc<WebState>>, Json(body): Json<DoorStart>) -> Response {
    let s = match settings() {
        Ok(s) => s,
        Err(message) => return fail(StatusCode::SERVICE_UNAVAILABLE, "not_configured", &message),
    };
    let label: String = body.label.chars().filter(|c| !c.is_control()).take(64).collect();
    match wdk_call(&state, &s, reqwest::Method::POST, "/door/attempts", Some(json!({"label": label}))).await {
        Ok(mut attempt) => {
            attempt["loginRequired"] = Value::Bool(state.auth.enabled());
            Json(attempt).into_response()
        }
        Err(response) => response,
    }
}

/// Poll an attempt. When it is paid, hand out the session token (once).
pub async fn door_check(
    Extension(state): Extension<Arc<WebState>>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Response {
    let s = match settings() {
        Ok(s) => s,
        Err(message) => return fail(StatusCode::SERVICE_UNAVAILABLE, "not_configured", &message),
    };
    let mut attempt = match wdk_call(&state, &s, reqwest::Method::GET, &format!("/door/attempts/{id}"), None).await {
        Ok(v) => v,
        Err(response) => return response,
    };
    attempt["loginRequired"] = Value::Bool(state.auth.enabled());
    let paid = attempt["status"] == "paid" && attempt["pass"]["active"] == true;
    if paid && state.auth.enabled() {
        let token = {
            let mut tokens = door_tokens().lock().expect("door tokens");
            tokens
                .entry(id)
                .or_insert_with(|| {
                    let who = format!(
                        "pass:{}",
                        attempt["address"]
                            .as_str()
                            .unwrap_or("?")
                            .chars()
                            .rev()
                            .take(6)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>()
                    );
                    state.auth.issue_with_ttl(&who, super::auth::DEPLOYER_PERMISSIONS, PASS_TTL).token
                })
                .clone()
        };
        attempt["accessToken"] = Value::String(token);
        attempt["expiresIn"] = Value::from(PASS_TTL.as_secs());
    }
    Json(attempt).into_response()
}

#[derive(Debug, Deserialize)]
pub struct DoorPay {
    #[serde(rename = "attemptId")]
    attempt_id: u64,
}

/// Demo only: pay the door from the organizer wallet (a capped, rate-limited plain-sats send).
/// In a real deployment the organizer pays from their own wallet; this button exists so the
/// whole loop runs on one laptop.
/// Sats the demo button may spend per UTC day (`POWEREDGE_DOOR_DEMO_DAILY_SAT`, default 10,000;
/// 0 turns the button off). The button is reachable before a pass, so on a shared network it
/// must not be able to drain the organizer wallet.
fn demo_budget() -> &'static Mutex<(u64, u64)> {
    static SPENT: OnceLock<Mutex<(u64, u64)>> = OnceLock::new(); // (utc day, sats spent)
    SPENT.get_or_init(|| Mutex::new((0, 0)))
}

fn reserve_demo_budget(amount: u64) -> Result<(), String> {
    let cap: u64 = std::env::var("POWEREDGE_DOOR_DEMO_DAILY_SAT").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(10_000);
    if cap == 0 {
        return Err("the demo payment button is turned off on this server; pay from your own Mutinynet wallet".to_owned());
    }
    let day = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0);
    let mut spent = demo_budget().lock().expect("demo budget");
    if spent.0 != day {
        *spent = (day, 0);
    }
    if spent.1 + amount > cap {
        return Err(format!("today's demo budget ({cap} sats) is used up; pay from your own Mutinynet wallet"));
    }
    spent.1 += amount;
    Ok(())
}

fn release_demo_budget(amount: u64) {
    let mut spent = demo_budget().lock().expect("demo budget");
    spent.1 = spent.1.saturating_sub(amount);
}

pub async fn door_pay(Extension(state): Extension<Arc<WebState>>, Json(body): Json<DoorPay>) -> Response {
    let s = match settings() {
        Ok(s) => s,
        Err(message) => return fail(StatusCode::SERVICE_UNAVAILABLE, "not_configured", &message),
    };
    let attempt =
        match wdk_call(&state, &s, reqwest::Method::GET, &format!("/door/attempts/{}", body.attempt_id), None).await {
            Ok(v) => v,
            Err(response) => return response,
        };
    if attempt["status"] != "pending" {
        return fail(
            StatusCode::CONFLICT,
            "attempt_not_pending",
            "this door attempt is no longer waiting for a payment",
        );
    }
    let (Some(address), Some(amount)) = (attempt["address"].as_str(), attempt["amountSat"].as_u64()) else {
        return fail(StatusCode::BAD_GATEWAY, "door_error", "the door attempt has no address");
    };
    let url = format!("{}/btc/pay-small", s.payer_url);
    let client = match http_client(&state, EgressPurpose::HttpNode, &url).await {
        Ok(c) => c,
        Err(response) => return response,
    };
    if let Err(message) = reserve_demo_budget(amount) {
        return fail(StatusCode::TOO_MANY_REQUESTS, "demo_budget_spent", &message);
    }
    let payload = json!({"address": address, "amount_sat": amount, "fee_rate": 2, "purpose": format!("door pass #{}", body.attempt_id)});
    match client.post(&url).bearer_auth(&s.wallet_token).json(&payload).timeout(Duration::from_secs(180)).send().await {
        Ok(reply) => {
            let status = StatusCode::from_u16(reply.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            if !status.is_success() {
                release_demo_budget(amount); // nothing was spent
            }
            let value: Value = reply.json().await.unwrap_or(Value::Null);
            (status, Json(value)).into_response()
        }
        Err(e) => {
            release_demo_budget(amount);
            fail(StatusCode::BAD_GATEWAY, "wallet_unavailable", &format!("organizer wallet not reachable: {e}"))
        }
    }
}
