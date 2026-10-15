//! `rgb-wallet-ref` config node and the HTTP client the RGB nodes share.
//!
//! The wallet-owner service is the only process that holds keys. Nodes reach it over
//! loopback with a bearer token read from an operator file (`POWEREDGE_WALLET_TOKEN_FILE`),
//! so no secret ever appears in `flows.json`, the editor, or a message.

use std::time::Duration;

use reqwest::Method;
use serde_json::{Value, json};

use crate::N2linkError;
use crate::runtime::egress::{EgressPolicyHandle, EgressPurpose};
use crate::runtime::engine::Engine;
use crate::runtime::flow::Flow;
use crate::runtime::model::ElementId;
use crate::runtime::model::json::RedGlobalNodeConfig;
use crate::runtime::nodes::*;
use n2link_macro::*;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const CALL_TIMEOUT: Duration = Duration::from_secs(300);

crate::node_hints!("rgb-wallet-ref", caps = ["wallet", "network"]);

#[global_node("rgb-wallet-ref", red_name = "rgb-wallet-ref")]
pub struct RgbWalletRefNode {
    base: BaseGlobalNodeState,
    pub(crate) base_url: String,
    pub(crate) wallet_id: String,
    token: String,
    egress: EgressPolicyHandle,
}

impl RgbWalletRefNode {
    fn build(
        engine: &Engine,
        config: &RedGlobalNodeConfig,
        _options: Option<&config::Config>,
    ) -> crate::Result<Box<dyn GlobalNodeBehavior>> {
        let field = |name: &str| config.rest.get(name).and_then(Value::as_str).unwrap_or_default().trim().to_owned();
        let base_url = loopback_url(&field("baseUrl"))?;
        let wallet_id = field("walletId");
        if !matches!(wallet_id.as_str(), "payer" | "recipient") {
            return Err(N2linkError::invalid_operation("rgb-wallet-ref walletId must be payer or recipient"));
        }
        // Validated, not stored: the wallet service itself refuses anything but its test network.
        let environment = field("environment");
        if !matches!(environment.as_str(), "mutinynet" | "regtest") {
            return Err(N2linkError::invalid_operation("rgb-wallet-ref environment must be a test network"));
        }
        Ok(Box::new(RgbWalletRefNode {
            base: BaseGlobalNodeState {
                id: config.id,
                name: config.name.clone(),
                type_str: "rgb-wallet-ref",
                ordering: config.ordering,
                context: engine.get_context_manager().new_context(engine.context(), config.id.to_string()),
                disabled: config.disabled,
            },
            base_url,
            wallet_id,
            token: read_token()?,
            egress: engine.egress_policy().clone(),
        }))
    }
}

#[async_trait::async_trait]
impl GlobalNodeBehavior for RgbWalletRefNode {
    fn get_base(&self) -> &BaseGlobalNodeState {
        &self.base
    }
}

/// Only `http://127.0.0.1|localhost|[::1]:<port>`: the wallet service never leaves the host.
fn loopback_url(raw: &str) -> crate::Result<String> {
    let url =
        reqwest::Url::parse(raw).map_err(|_| N2linkError::invalid_operation("rgb-wallet-ref baseUrl is not a URL"))?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if url.scheme() != "http" || !loopback || url.port().is_none() || url.path() != "/" || url.query().is_some() {
        return Err(N2linkError::invalid_operation("rgb-wallet-ref baseUrl must be http://127.0.0.1:<port>"));
    }
    Ok(raw.trim_end_matches('/').to_owned())
}

fn read_token() -> crate::Result<String> {
    let path = std::env::var("POWEREDGE_WALLET_TOKEN_FILE")
        .map_err(|_| N2linkError::invalid_operation("POWEREDGE_WALLET_TOKEN_FILE is not set"))?;
    let token = std::fs::read_to_string(&path)
        .map_err(|_| N2linkError::invalid_operation("wallet API token file is not readable"))?
        .trim()
        .to_owned();
    if token.len() < 32 {
        return Err(N2linkError::invalid_operation("wallet API token is too short"));
    }
    Ok(token)
}

/// What a node needs to call one wallet; cloned out so no lock spans network I/O.
#[derive(Clone)]
pub(crate) struct WalletHandle {
    base_url: String,
    token: String,
    egress: EgressPolicyHandle,
}

/// `role` is the wallet this node is allowed to use ("payer" or "recipient").
pub(crate) fn wallet_from_flow(flow: &Flow, reference: &str, role: &str) -> crate::Result<WalletHandle> {
    let id: ElementId =
        reference.parse().map_err(|_| N2linkError::invalid_operation("RGB node wallet reference is not a node id"))?;
    let engine = flow.engine().ok_or_else(|| N2linkError::invalid_operation("RGB node has no engine"))?;
    let global = engine
        .find_global_node_by_id(&id)
        .ok_or_else(|| N2linkError::invalid_operation(&format!("rgb-wallet-ref '{id}' was not loaded")))?;
    let node = global
        .as_any()
        .downcast_ref::<RgbWalletRefNode>()
        .ok_or_else(|| N2linkError::invalid_operation(&format!("node '{id}' is not an rgb-wallet-ref")))?;
    if node.wallet_id != role {
        return Err(N2linkError::invalid_operation(&format!(
            "this node must use the {role} wallet, not the {} wallet",
            node.wallet_id
        )));
    }
    Ok(WalletHandle { base_url: node.base_url.clone(), token: node.token.clone(), egress: node.egress.clone() })
}

/// A wallet call that failed. `code` is the service's stable code (or a transport code).
#[derive(Debug, Clone)]
pub(crate) struct CallError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl CallError {
    pub(crate) fn transport(message: impl Into<String>) -> Self {
        CallError { status: 503, code: "wallet_unreachable".into(), message: message.into() }
    }

    pub(crate) fn to_json(&self, step: &str) -> Value {
        json!({ "ok": false, "step": step, "error": { "code": self.code, "message": self.message } })
    }
}

impl WalletHandle {
    pub(crate) async fn call(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value, CallError> {
        let url = format!("{}{}", self.base_url, path);
        let policy = self.egress.snapshot();
        let client = policy
            .http_client(EgressPurpose::HttpNode, &url)
            .await
            .map_err(|err| CallError::transport(format!("egress denied: {err}")))?;
        let mut request = client.request(method, &url).bearer_auth(&self.token).timeout(CALL_TIMEOUT);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|err| CallError::transport(err.to_string()))?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|err| CallError::transport(err.to_string()))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(CallError::transport("wallet response is too large"));
        }
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(value);
        }
        let text = |p: &str| value.pointer(p).and_then(Value::as_str).unwrap_or_default().to_owned();
        let code = text("/error/code");
        Err(CallError {
            status,
            code: if code.is_empty() { format!("http_{status}") } else { code },
            message: text("/error/message"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::loopback_url;

    #[test]
    fn only_loopback_http_with_port_is_accepted() {
        assert!(loopback_url("http://127.0.0.1:8711").is_ok());
        assert!(loopback_url("http://localhost:8712/").is_ok());
        for bad in [
            "https://127.0.0.1:8711",
            "http://10.0.0.5:8711",
            "http://wallet.example.com:8711",
            "http://127.0.0.1",
            "http://127.0.0.1:8711/admin",
            "http://127.0.0.1:8711/?x=1",
            "not a url",
        ] {
            assert!(loopback_url(bad).is_err(), "{bad}");
        }
    }
}
