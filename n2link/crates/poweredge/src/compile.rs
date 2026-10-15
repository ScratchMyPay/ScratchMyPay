//! Deterministic compiler: (template, intent, operator context) -> n2link flow nodes.
//!
//! Placeholders in `graph.json` must be the *whole* string value:
//! - `{{slot:<name>}}`  bound intent parameter (typed; amounts also as `<name>_base_units`)
//! - `{{ctx:<key>}}`    operator context (wallet URLs etc.), never from the model
//! - `{{ref:<ref>}}`    ID of another node in the template (config references)
//! - `{{workspace}}`    target editor tab ID
//! - `{{environment}}`  the intent's environment
//! - `{{template_digest}}` the pinned template digest (lets nodes prove their origin)
//!
//! Any other occurrence of `{{` is an error, so model text can never be interpolated
//! into a larger string.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::canonical;
use crate::intent::{ParamValue, WorkflowIntent};
use crate::registry::{SlotKind, SlotSpec, Template, TemplateRegistry};
use crate::validate::validate;
use crate::{Error, Result};

const DEFAULT_TEXT_CHARS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetInfo {
    /// Decimal places, read from the payer wallet (never from the model).
    pub precision: u8,
}

#[derive(Debug, Clone, Default)]
pub struct BindContext {
    pub workspace_id: String,
    pub values: BTreeMap<String, String>,
    /// Assets the payer wallet holds, keyed by ticker.
    pub assets: BTreeMap<String, AssetInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFlow {
    pub template_id: String,
    pub template_version: String,
    pub template_digest: String,
    pub intent_digest: String,
    /// SHA-256 of the canonical node array; same inputs always give the same value.
    pub flow_digest: String,
    pub nodes: Vec<Value>,
    /// Typed values actually bound into the graph (for the plain-language review card).
    pub bound: BTreeMap<String, Value>,
}

pub fn compile(registry: &TemplateRegistry, intent: &WorkflowIntent, ctx: &BindContext) -> Result<CompiledFlow> {
    intent.check()?;
    let template = registry.resolve(&intent.template_id, &intent.template_version)?;
    let manifest = &template.manifest;
    if !manifest.allowed_environments.contains(&intent.environment) {
        return Err(Error::Environment(intent.environment.as_str().to_owned()));
    }
    if ctx.workspace_id.is_empty() || !ctx.workspace_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Context("workspace_id".into()));
    }
    for key in &manifest.context_keys {
        if ctx.values.get(key).is_none_or(String::is_empty) {
            return Err(Error::Context(key.clone()));
        }
    }

    let bound = bind_slots(template, intent, ctx)?;
    let intent_digest = intent.digest();
    let ids = node_ids(template, &intent_digest, &ctx.workspace_id)?;
    let subst = Substitution { bound: &bound, ctx, ids: &ids, intent, template_digest: template.digest() };

    let mut nodes = Vec::with_capacity(template.graph.len());
    for node in &template.graph {
        nodes.push(subst.node(node)?);
    }
    validate(manifest, &nodes, intent.environment)?;

    Ok(CompiledFlow {
        template_id: manifest.id.clone(),
        template_version: manifest.version.clone(),
        template_digest: manifest.digest.clone(),
        intent_digest,
        flow_digest: canonical::digest(&Value::Array(nodes.clone())),
        nodes,
        bound,
    })
}

fn bind_slots(template: &Template, intent: &WorkflowIntent, ctx: &BindContext) -> Result<BTreeMap<String, Value>> {
    let slots = &template.manifest.slots;
    if let Some(unknown) = intent.parameters.keys().find(|name| !slots.contains_key(*name)) {
        return Err(Error::param(unknown, "is not a parameter of this template"));
    }
    let mut bound = BTreeMap::new();
    // Tickers first: amounts need their precision.
    let mut order: Vec<(&String, &SlotSpec)> = slots.iter().collect();
    order.sort_by_key(|(_, spec)| spec.kind != SlotKind::Ticker);
    for (name, spec) in order {
        let value = intent.parameters.get(name);
        let Some(value) = value else {
            if spec.required {
                return Err(Error::param(name, "is required"));
            }
            bound.insert(name.clone(), default_for(spec.kind));
            continue;
        };
        match spec.kind {
            SlotKind::Text => {
                let text = expect_text(name, value)?;
                let max = spec.max_len.unwrap_or(DEFAULT_TEXT_CHARS);
                if text.chars().count() > max {
                    return Err(Error::param(name, format!("is longer than {max} characters")));
                }
                bound.insert(name.clone(), Value::String(text.trim().to_owned()));
            }
            SlotKind::Ticker => {
                // RGB tickers are uppercase (rgb-lib rule); people write "tUSDT". Deterministic
                // normalization, then an exact match against what the wallet actually holds.
                let ticker = expect_text(name, value)?.trim().to_ascii_uppercase();
                if !ctx.assets.contains_key(&ticker) {
                    return Err(Error::param(name, format!("asset `{ticker}` is not held by the payer wallet")));
                }
                bound.insert(name.clone(), Value::String(ticker));
            }
            SlotKind::DecimalAmount => {
                let unit = spec.unit_slot.as_deref().unwrap_or_default();
                let ticker = bound.get(unit).and_then(Value::as_str).unwrap_or_default();
                let precision = ctx
                    .assets
                    .get(ticker)
                    .ok_or_else(|| Error::param(name, "has no known asset to measure it in"))?
                    .precision;
                let display = match value {
                    ParamValue::Text(text) => text.trim().to_owned(),
                    _ => {
                        return Err(Error::param(name, "must be a decimal string in display units, for example \"25\""))
                    }
                };
                let base = to_base_units(&display, precision).map_err(|r| Error::param(name, r))?;
                bound.insert(name.clone(), Value::String(display));
                bound.insert(format!("{name}_base_units"), json!(base));
            }
            SlotKind::Bool => match value {
                ParamValue::Bool(flag) => {
                    bound.insert(name.clone(), Value::Bool(*flag));
                }
                _ => return Err(Error::param(name, "must be true or false")),
            },
        }
    }
    Ok(bound)
}

fn default_for(kind: SlotKind) -> Value {
    match kind {
        SlotKind::Bool => Value::Bool(false),
        _ => Value::String(String::new()),
    }
}

fn expect_text<'a>(name: &str, value: &'a ParamValue) -> Result<&'a str> {
    match value {
        ParamValue::Text(text) => Ok(text.as_str()),
        _ => Err(Error::param(name, "must be text")),
    }
}

/// "25" or "12.5" in display units -> integer base units. No signs, exponents or rounding.
pub fn to_base_units(display: &str, precision: u8) -> std::result::Result<u64, String> {
    let (whole, frac) = display.split_once('.').unwrap_or((display, ""));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || (display.contains('.') && !digits(frac)) {
        return Err(format!("`{display}` is not a plain decimal amount"));
    }
    if frac.len() > usize::from(precision) {
        return Err(format!("has more than {precision} decimal places"));
    }
    let scale = 10u64.checked_pow(u32::from(precision)).ok_or("precision is too large")?;
    let whole: u64 = whole.parse().map_err(|_| "is too large".to_owned())?;
    let frac_padded = format!("{frac:0<width$}", width = usize::from(precision));
    let frac: u64 = if frac_padded.is_empty() { 0 } else { frac_padded.parse().map_err(|_| "is malformed")? };
    let base = whole.checked_mul(scale).and_then(|w| w.checked_add(frac)).ok_or_else(|| "is too large".to_owned())?;
    if base == 0 {
        return Err("must be greater than zero".into());
    }
    Ok(base)
}

fn node_ids(template: &Template, intent_digest: &str, workspace: &str) -> Result<BTreeMap<String, String>> {
    let mut ids = BTreeMap::new();
    for node in &template.graph {
        let reference = node
            .get("ref")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Integrity("every template node needs a string `ref`".into()))?;
        // Config nodes (no `z`) get ids independent of the intent, so flows on other tabs that
        // reference a wallet keep working when the app prepares the next payout.
        let per_intent = if node.get("z").is_some() { intent_digest } else { "config" };
        let seed = format!("{}\n{per_intent}\n{workspace}\n{reference}", template.digest());
        let id = canonical::sha256_hex(seed.as_bytes())[..16].to_owned();
        if ids.insert(reference.to_owned(), id).is_some() {
            return Err(Error::Integrity(format!("duplicate template ref `{reference}`")));
        }
    }
    Ok(ids)
}

struct Substitution<'a> {
    bound: &'a BTreeMap<String, Value>,
    ctx: &'a BindContext,
    ids: &'a BTreeMap<String, String>,
    intent: &'a WorkflowIntent,
    template_digest: &'a str,
}

impl Substitution<'_> {
    fn node(&self, template_node: &Value) -> Result<Value> {
        let Value::Object(source) = template_node else {
            return Err(Error::Integrity("template node must be an object".into()));
        };
        let mut out = Map::new();
        for (key, value) in source {
            match key.as_str() {
                "ref" => {
                    let reference = value.as_str().unwrap_or_default();
                    out.insert("id".into(), Value::String(self.ids[reference].clone()));
                }
                "wires" => {
                    out.insert("wires".into(), self.wires(value)?);
                }
                _ => {
                    out.insert(key.clone(), self.value(value)?);
                }
            }
        }
        Ok(Value::Object(out))
    }

    fn wires(&self, value: &Value) -> Result<Value> {
        let ports = value.as_array().ok_or_else(|| Error::Integrity("wires must be an array".into()))?;
        let mut out = Vec::with_capacity(ports.len());
        for port in ports {
            let targets = port.as_array().ok_or_else(|| Error::Integrity("wire port must be an array".into()))?;
            let mut ids = Vec::with_capacity(targets.len());
            for target in targets {
                let reference = target.as_str().unwrap_or_default();
                let id = self
                    .ids
                    .get(reference)
                    .ok_or_else(|| Error::Integrity(format!("wire to unknown ref `{reference}`")))?;
                ids.push(Value::String(id.clone()));
            }
            out.push(Value::Array(ids));
        }
        Ok(Value::Array(out))
    }

    fn value(&self, value: &Value) -> Result<Value> {
        match value {
            Value::String(text) => self.string(text),
            Value::Array(items) => items.iter().map(|v| self.value(v)).collect::<Result<_>>().map(Value::Array),
            Value::Object(map) => {
                let mut out = Map::new();
                for (k, v) in map {
                    out.insert(k.clone(), self.value(v)?);
                }
                Ok(Value::Object(out))
            }
            other => Ok(other.clone()),
        }
    }

    fn string(&self, text: &str) -> Result<Value> {
        if !text.contains("{{") {
            return Ok(Value::String(text.to_owned()));
        }
        let inner = text
            .strip_prefix("{{")
            .and_then(|t| t.strip_suffix("}}"))
            .ok_or_else(|| Error::Integrity(format!("partial placeholder in `{text}`")))?;
        let missing = |what: &str| Error::Integrity(format!("unknown placeholder {what} in `{text}`"));
        match inner.split_once(':') {
            Some(("slot", name)) => self.bound.get(name).cloned().ok_or_else(|| missing("slot")),
            Some(("ctx", key)) => self.ctx.values.get(key).cloned().map(Value::String).ok_or_else(|| missing("ctx")),
            Some(("ref", reference)) => {
                self.ids.get(reference).cloned().map(Value::String).ok_or_else(|| missing("ref"))
            }
            None if inner == "workspace" => Ok(Value::String(self.ctx.workspace_id.clone())),
            None if inner == "environment" => Ok(Value::String(self.intent.environment.as_str().to_owned())),
            None if inner == "template_digest" => Ok(Value::String(self.template_digest.to_owned())),
            _ => Err(missing("kind")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_base_units;

    #[test]
    fn decimal_amounts_convert_exactly() {
        assert_eq!(to_base_units("25", 6), Ok(25_000_000));
        assert_eq!(to_base_units("12.5", 6), Ok(12_500_000));
        assert_eq!(to_base_units("0.000001", 6), Ok(1));
        assert_eq!(to_base_units("7", 0), Ok(7));
    }

    #[test]
    fn ambiguous_or_unsafe_amounts_are_rejected() {
        for bad in ["", "-5", "+5", "1e3", "25.", ".5", "0", "0.0", "1.0000001", "25 ", "2,5", "18446744073710"] {
            assert!(to_base_units(bad, 6).is_err(), "accepted `{bad}`");
        }
    }
}
