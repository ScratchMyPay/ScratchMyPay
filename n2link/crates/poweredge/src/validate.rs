//! Fail-closed graph validator, independent of the compiler.
//!
//! Runs on compiler output and can run again on whatever is about to be deployed, so a
//! hand-edited or tampered graph is held to the same rules.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value;

use crate::intent::Environment;
use crate::registry::Manifest;
use crate::{Error, Result};

const MAX_NODES: usize = 64;

/// Never allowed in a PowerEdge graph, whatever a manifest says (defense in depth).
fn denied_type(node_type: &str) -> bool {
    let t = node_type.to_ascii_lowercase();
    matches!(t.as_str(), "exec" | "function" | "link in" | "link out" | "link call" | "subflow")
        || t.starts_with("wasm")
        || t.starts_with("subflow:")
        || t.contains("python")
}

/// Keys that would mean a secret is sitting in flows.json instead of the credential store.
fn sensitive_key(key: &str) -> bool {
    let k: String = key.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    ["password", "passphrase", "apikey", "mnemonic", "seed", "privatekey", "secret", "token", "xprv", "tprv"]
        .iter()
        .any(|word| k.contains(word))
}

pub fn validate(manifest: &Manifest, nodes: &[Value], environment: Environment) -> Result<()> {
    if !manifest.allowed_environments.contains(&environment) {
        return Err(Error::Environment(environment.as_str().to_owned()));
    }
    if nodes.is_empty() || nodes.len() > MAX_NODES {
        return Err(Error::Graph(format!("graph must have 1..={MAX_NODES} nodes")));
    }

    let mut types: BTreeMap<&str, &str> = BTreeMap::new();
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for node in nodes {
        let object = node.as_object().ok_or_else(|| Error::Graph("node is not an object".into()))?;
        let id = object.get("id").and_then(Value::as_str).ok_or_else(|| Error::Graph("node without id".into()))?;
        let node_type =
            object.get("type").and_then(Value::as_str).ok_or_else(|| Error::Graph(format!("node {id} has no type")))?;
        if denied_type(node_type) {
            return Err(Error::Graph(format!("node type `{node_type}` is never allowed")));
        }
        if !manifest.allowed_node_types.iter().any(|t| t == node_type) {
            return Err(Error::Graph(format!("node type `{node_type}` is not allowed by {}", manifest.id)));
        }
        if types.insert(id, node_type).is_some() {
            return Err(Error::Graph(format!("duplicate node id {id}")));
        }
        scan_values(id, node)?;
        if node_type == "http in" {
            let host = object.get("host").and_then(Value::as_str).unwrap_or("0.0.0.0");
            if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
                return Err(Error::Graph(format!("http endpoint {id} must listen on 127.0.0.1, not {host}")));
            }
        }
        let mut targets = Vec::new();
        if let Some(wires) = object.get("wires") {
            for port in wires.as_array().ok_or_else(|| Error::Graph(format!("node {id} wires malformed")))? {
                for target in port.as_array().ok_or_else(|| Error::Graph(format!("node {id} wire port malformed")))? {
                    targets
                        .push(target.as_str().ok_or_else(|| Error::Graph(format!("node {id} wire target malformed")))?);
                }
            }
        }
        edges.insert(id, targets);
    }

    for (id, targets) in &edges {
        if let Some(missing) = targets.iter().find(|t| !types.contains_key(**t)) {
            return Err(Error::Graph(format!("node {id} is wired to missing node {missing}")));
        }
    }
    check_approval_paths(manifest, &types, &edges)
}

/// Recursively reject unresolved placeholders and secret-looking keys.
fn scan_values(id: &str, value: &Value) -> Result<()> {
    match value {
        Value::String(text) if text.contains("{{") => {
            Err(Error::Graph(format!("node {id} has an unresolved placeholder")))
        }
        Value::Array(items) => items.iter().try_for_each(|v| scan_values(id, v)),
        Value::Object(map) => {
            for (key, inner) in map {
                if sensitive_key(key) {
                    return Err(Error::Graph(format!("node {id} carries a secret-looking field `{key}`")));
                }
                scan_values(id, inner)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// From every entry node, walk the graph without passing through approval nodes.
/// Reaching a guarded (spending) node means approval can be bypassed.
fn check_approval_paths(
    manifest: &Manifest,
    types: &BTreeMap<&str, &str>,
    edges: &BTreeMap<&str, Vec<&str>>,
) -> Result<()> {
    let is = |id: &str, wanted: &[String]| wanted.iter().any(|t| Some(t.as_str()) == types.get(id).copied());
    let guarded: BTreeSet<&str> = types.keys().copied().filter(|id| is(id, &manifest.guarded_node_types)).collect();
    if guarded.is_empty() {
        return Ok(());
    }
    if !types.values().any(|t| *t == manifest.approval_node_type) {
        return Err(Error::Graph("spending node present but no approval gate".into()));
    }
    // Entry points: declared entry types plus any node nobody wires into.
    let has_inbound: BTreeSet<&str> = edges.values().flatten().copied().collect();
    let entries: Vec<&str> = types
        .keys()
        .copied()
        .filter(|id| is(id, &manifest.entry_node_types) || !has_inbound.contains(id))
        .filter(|id| types[id] != manifest.approval_node_type)
        .collect();

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut queue: VecDeque<(&str, &str)> = entries.iter().map(|e| (*e, *e)).collect();
    while let Some((node, origin)) = queue.pop_front() {
        if guarded.contains(node) {
            return Err(Error::Graph(format!("path from {origin} reaches spending node {node} without approval")));
        }
        if !seen.insert(node) {
            continue;
        }
        for &next in &edges[node] {
            if types[next] != manifest.approval_node_type {
                queue.push_back((next, origin));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{denied_type, sensitive_key};

    #[test]
    fn deny_list_covers_code_execution() {
        for t in
            ["exec", "function", "Function", "wasm-plugin", "wasm:hash", "python-function", "link out", "subflow:abc"]
        {
            assert!(denied_type(t), "{t}");
        }
        assert!(!denied_type("rgb-send-once"));
    }

    #[test]
    fn secret_like_keys_are_detected() {
        for k in ["password", "apiKey", "api_key", "mnemonic", "seedPhrase", "privateKey", "accessToken"] {
            assert!(sensitive_key(k), "{k}");
        }
        assert!(!sensitive_key("walletId"));
    }
}
