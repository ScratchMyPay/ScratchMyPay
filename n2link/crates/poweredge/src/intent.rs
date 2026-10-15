//! `WorkflowIntent v1`: the only thing the local model is allowed to produce.
//!
//! Untrusted input. Parsing is strict (unknown fields, floats, oversize, control characters
//! and malformed identifiers are rejected) and never "repairs" model output.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

pub const MAX_INTENT_BYTES: usize = 8 * 1024;
const MAX_PARAMS: usize = 16;
const MAX_STRING_CHARS: usize = 256;
const MAX_RATIONALE_CHARS: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Mutinynet,
    Regtest,
}

impl Environment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Environment::Mutinynet => "mutinynet",
            Environment::Regtest => "regtest",
        }
    }
}

/// Parameter values: no floats (amounts travel as decimal strings), no nesting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Integer(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowIntent {
    pub version: u8,
    pub template_id: String,
    pub template_version: String,
    pub environment: Environment,
    pub parameters: BTreeMap<String, ParamValue>,
    /// Model explanation. Shown to the user as non-authoritative; never compiled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}

impl WorkflowIntent {
    /// Parse and check untrusted bytes. Fails closed on any deviation.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_INTENT_BYTES {
            return Err(Error::Intent(format!("intent exceeds {MAX_INTENT_BYTES} bytes")));
        }
        let intent: Self = serde_json::from_slice(bytes).map_err(|err| Error::Intent(err.to_string()))?;
        intent.check()?;
        Ok(intent)
    }

    pub fn check(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::Intent("version must be 1".into()));
        }
        if !valid_template_id(&self.template_id) {
            return Err(Error::Intent("template_id is malformed".into()));
        }
        if !valid_semver(&self.template_version) {
            return Err(Error::Intent("template_version must be MAJOR.MINOR.PATCH".into()));
        }
        if self.parameters.len() > MAX_PARAMS {
            return Err(Error::Intent(format!("more than {MAX_PARAMS} parameters")));
        }
        for (name, value) in &self.parameters {
            if !valid_param_name(name) {
                return Err(Error::Intent(format!("parameter name `{name}` is malformed")));
            }
            if let ParamValue::Text(text) = value {
                check_text(name, text, MAX_STRING_CHARS)?;
            }
        }
        if let Some(rationale) = &self.rationale {
            if rationale.chars().count() > MAX_RATIONALE_CHARS {
                return Err(Error::Intent("rationale is too long".into()));
            }
        }
        Ok(())
    }

    /// Authoritative fields only (rationale excluded), so explanation wording can never
    /// change node IDs or digests.
    pub fn canonical_value(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("intent always serializes");
        if let Some(object) = value.as_object_mut() {
            object.remove("rationale");
        }
        value
    }

    pub fn digest(&self) -> String {
        crate::canonical::digest(&self.canonical_value())
    }
}

fn check_text(name: &str, text: &str, max_chars: usize) -> Result<()> {
    if text.chars().count() > max_chars {
        return Err(Error::Intent(format!("parameter `{name}` is too long")));
    }
    if text.chars().any(char::is_control) {
        return Err(Error::Intent(format!("parameter `{name}` contains control characters")));
    }
    Ok(())
}

pub(crate) fn valid_template_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (3..=64).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

pub(crate) fn valid_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| !part.is_empty() && part.len() <= 6 && part.bytes().all(|b| b.is_ascii_digit()))
}

fn valid_param_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERO: &str = r#"{"version":1,"template_id":"rgb-payment-v1","template_version":"1.0.0",
        "environment":"mutinynet","parameters":{"recipient_label":"sound engineer",
        "amount":"25","asset_ticker":"tUSDT","memo":"Saturday's show"},
        "rationale":"Pay the sound engineer."}"#;

    #[test]
    fn hero_intent_parses() {
        let intent = WorkflowIntent::parse(HERO.as_bytes()).unwrap();
        assert_eq!(intent.environment, Environment::Mutinynet);
        assert_eq!(intent.parameters["amount"], ParamValue::Text("25".into()));
    }

    #[test]
    fn rationale_does_not_change_the_digest() {
        let a = WorkflowIntent::parse(HERO.as_bytes()).unwrap();
        let mut b = a.clone();
        b.rationale = Some("Completely different words.".into());
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn rejects_unknown_fields_mainnet_floats_and_bad_versions() {
        let cases = [
            HERO.replace(r#""version":1,"#, r#""version":1,"nodes":[],"#),
            HERO.replace("mutinynet", "mainnet"),
            HERO.replace(r#""amount":"25""#, r#""amount":25.5"#),
            HERO.replace(r#""version":1"#, r#""version":2"#),
            HERO.replace("1.0.0", "1.0"),
            HERO.replace("rgb-payment-v1", "RGB payment"),
            HERO.replace("sound engineer", "line\u{0007}bell"),
        ];
        for case in cases {
            assert!(WorkflowIntent::parse(case.as_bytes()).is_err(), "accepted: {case}");
        }
    }

    #[test]
    fn rejects_oversize_and_too_many_parameters() {
        let big = format!(r#"{{"pad":"{}"}}"#, "x".repeat(MAX_INTENT_BYTES));
        assert!(WorkflowIntent::parse(big.as_bytes()).is_err());
        let params: Vec<String> = (0..17).map(|i| format!(r#""p{i}":"v""#)).collect();
        let many = format!(
            r#"{{"version":1,"template_id":"rgb-payment-v1","template_version":"1.0.0","environment":"regtest","parameters":{{{}}}}}"#,
            params.join(",")
        );
        assert!(WorkflowIntent::parse(many.as_bytes()).is_err());
    }
}
