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
use poweredge::{AssetInfo, BindContext, CompiledFlow, SlotKind, TemplateRegistry, WorkflowIntent, compile};
use serde::Deserialize;
use serde_json::{Value, json};

use super::WebState;
use super::deploy;
use super::reply::api_error;

const APP_HTML: &str = include_str!("../../poweredge-app/index.html");
/// Fixed editor tab that holds the active PowerEdge payout flow.
pub const POWEREDGE_TAB: &str = "9e0d9e0d9e0d9e0d";
const MAX_PROMPT_CHARS: usize = 1_000;
const DRAFT_TTL: Duration = Duration::from_secs(30 * 60);
const QVAC_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_REPLY_BYTES: usize = 64 * 1024;

struct Settings {
    templates_dir: PathBuf,
    qvac_url: String,
    qvac_key: String,
    qvac_model: String,
    payer_url: String,
    recipient_url: String,
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
        payer_url: loopback(&env_or("POWEREDGE_PAYER_URL", "http://127.0.0.1:8711"), "payer wallet")?,
        recipient_url: loopback(&env_or("POWEREDGE_RECIPIENT_URL", "http://127.0.0.1:8712"), "recipient wallet")?,
        wallet_token: read_secret("POWEREDGE_WALLET_TOKEN_FILE")?,
    })
}

struct Draft {
    created: Instant,
    compiled: CompiledFlow,
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
RGB2link PowerEdge (post-show payouts to crew and artists).\n\
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

pub async fn get_app() -> Html<&'static str> {
    Html(APP_HTML)
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
        "ai": {"engine": "QVAC", "model": s.qvac_model, "endpoint": s.qvac_url, "local": true},
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
    let ctx = BindContext {
        workspace_id: POWEREDGE_TAB.into(),
        values: BTreeMap::from([
            ("payer_wallet_url".into(), s.payer_url.clone()),
            ("recipient_wallet_url".into(), s.recipient_url.clone()),
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
        },
        "ai": {"engine": "QVAC", "model": model, "latencyMs": latency_ms, "local": true, "explanation": rationale},
        "template": {"id": compiled.template_id, "version": compiled.template_version, "digest": compiled.template_digest},
        "flow": {"digest": compiled.flow_digest, "intentDigest": compiled.intent_digest, "nodes": compiled.nodes.len()},
        "intent": intent,
    });
    {
        let mut store = drafts().lock().unwrap_or_else(|p| p.into_inner());
        store.retain(|_, d| d.created.elapsed() < DRAFT_TTL);
        store.insert(draft_id, Draft { created: Instant::now(), compiled });
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
    let nodes = {
        let store = drafts().lock().unwrap_or_else(|p| p.into_inner());
        match store.get(&request.draft_id) {
            Some(d) if d.created.elapsed() < DRAFT_TTL => d.compiled.nodes.clone(),
            _ => return fail(StatusCode::NOT_FOUND, "draft_expired", "this payout draft expired; describe it again"),
        }
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
        .filter(|n| n["id"] != POWEREDGE_TAB && n["z"] != POWEREDGE_TAB && n["type"] != "rgb-wallet-ref")
        .collect();
    flows.push(json!({"id": POWEREDGE_TAB, "type": "tab", "label": "PowerEdge payouts", "info": "Managed by the PowerEdge app. Editing it voids pending approvals."}));
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
        return fail(StatusCode::NOT_FOUND, "unknown_endpoint", "no such PowerEdge step");
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
