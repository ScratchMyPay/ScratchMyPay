//! Pinned template registry: `templates/<id>/manifest.json` + `graph.json`.
//!
//! The manifest's `digest` must equal SHA-256 of the canonical JSON of
//! `{"manifest": <manifest without digest>, "graph": <graph>}`. Any mismatch fails loading.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::canonical;
use crate::intent::{valid_semver, valid_template_id, Environment};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlotKind {
    /// Free text, bounded by `max_len`.
    Text,
    /// Asset ticker; must be known to the payer wallet (from the bind context).
    Ticker,
    /// Decimal string in display units ("25", "12.5"); converted with wallet precision.
    DecimalAmount,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotSpec {
    pub kind: SlotKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub max_len: Option<usize>,
    /// For `decimal-amount`: the ticker slot whose precision applies.
    #[serde(default)]
    pub unit_slot: Option<String>,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub title: String,
    pub description: String,
    pub allowed_environments: Vec<Environment>,
    pub slots: BTreeMap<String, SlotSpec>,
    pub context_keys: Vec<String>,
    pub allowed_node_types: Vec<String>,
    pub approval_node_type: String,
    pub guarded_node_types: Vec<String>,
    pub entry_node_types: Vec<String>,
    pub digest: String,
}

#[derive(Debug, Clone)]
pub struct Template {
    pub manifest: Manifest,
    pub graph: Vec<Value>,
}

impl Template {
    pub fn digest(&self) -> &str {
        &self.manifest.digest
    }
}

#[derive(Debug, Default)]
pub struct TemplateRegistry {
    templates: BTreeMap<(String, String), Template>,
}

/// Digest of a template directory, computed the same way loading verifies it.
pub fn compute_dir_digest(dir: &Path) -> Result<String> {
    let (manifest, graph) = read_dir_values(dir)?;
    Ok(digest_of(&manifest, &graph))
}

fn digest_of(manifest: &Value, graph: &Value) -> String {
    let mut manifest = manifest.clone();
    if let Some(object) = manifest.as_object_mut() {
        object.remove("digest");
    }
    canonical::digest(&json!({ "manifest": manifest, "graph": graph }))
}

fn read_json(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).map_err(|err| Error::Io(format!("{}: {err}", path.display())))?;
    serde_json::from_str(&text).map_err(|err| Error::Integrity(format!("{}: {err}", path.display())))
}

fn read_dir_values(dir: &Path) -> Result<(Value, Value)> {
    Ok((read_json(&dir.join("manifest.json"))?, read_json(&dir.join("graph.json"))?))
}

impl TemplateRegistry {
    /// Load every template directory under `root`; any invalid template fails the whole load.
    pub fn load_dir(root: &Path) -> Result<Self> {
        let entries = fs::read_dir(root).map_err(|err| Error::Io(format!("{}: {err}", root.display())))?;
        let mut dirs: Vec<_> =
            entries.filter_map(|entry| entry.ok().map(|e| e.path())).filter(|path| path.is_dir()).collect();
        dirs.sort();
        let mut registry = TemplateRegistry::default();
        for dir in dirs {
            registry.insert(Self::load_one(&dir)?)?;
        }
        Ok(registry)
    }

    pub fn load_one(dir: &Path) -> Result<Template> {
        let (manifest_value, graph_value) = read_dir_values(dir)?;
        let manifest: Manifest = serde_json::from_value(manifest_value.clone())
            .map_err(|err| Error::Integrity(format!("manifest: {err}")))?;
        let expected = digest_of(&manifest_value, &graph_value);
        if manifest.digest != expected {
            return Err(Error::Integrity(format!(
                "{}@{} digest mismatch (manifest {}, computed {expected})",
                manifest.id, manifest.version, manifest.digest
            )));
        }
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if dir_name != manifest.id {
            return Err(Error::Integrity(format!(
                "directory `{dir_name}` does not match template id `{}`",
                manifest.id
            )));
        }
        let Value::Array(graph) = graph_value else {
            return Err(Error::Integrity("graph.json must be an array of nodes".into()));
        };
        Self::check_manifest(&manifest)?;
        Ok(Template { manifest, graph })
    }

    fn check_manifest(manifest: &Manifest) -> Result<()> {
        if !valid_template_id(&manifest.id) || !valid_semver(&manifest.version) {
            return Err(Error::Integrity("template id or version is malformed".into()));
        }
        if manifest.allowed_environments.is_empty() {
            return Err(Error::Integrity("template allows no environment".into()));
        }
        if !manifest.guarded_node_types.is_empty()
            && !manifest.allowed_node_types.contains(&manifest.approval_node_type)
        {
            return Err(Error::Integrity("guarded template without an approval node type".into()));
        }
        for (name, slot) in &manifest.slots {
            if slot.kind == SlotKind::DecimalAmount {
                let unit = slot.unit_slot.as_deref().unwrap_or_default();
                if manifest.slots.get(unit).map(|s| s.kind) != Some(SlotKind::Ticker) {
                    return Err(Error::Integrity(format!("amount slot `{name}` needs a ticker `unit_slot`")));
                }
            }
        }
        Ok(())
    }

    pub fn insert(&mut self, template: Template) -> Result<()> {
        let key = (template.manifest.id.clone(), template.manifest.version.clone());
        if self.templates.contains_key(&key) {
            return Err(Error::Integrity(format!("duplicate template {}@{}", key.0, key.1)));
        }
        self.templates.insert(key, template);
        Ok(())
    }

    /// Exact match on id and version. No "nearest version" fallback.
    pub fn resolve(&self, id: &str, version: &str) -> Result<&Template> {
        self.templates
            .get(&(id.to_owned(), version.to_owned()))
            .ok_or_else(|| Error::UnknownTemplate { id: id.to_owned(), version: version.to_owned() })
    }

    pub fn iter(&self) -> impl Iterator<Item = &Template> {
        self.templates.values()
    }

    /// What the model is told: ids, versions, purpose and slots. Never graphs or wallets.
    pub fn catalog(&self) -> Value {
        Value::Array(
            self.iter()
                .map(|t| {
                    json!({
                        "template_id": t.manifest.id,
                        "template_version": t.manifest.version,
                        "title": t.manifest.title,
                        "description": t.manifest.description,
                        "environments": t.manifest.allowed_environments,
                        "parameters": t.manifest.slots,
                    })
                })
                .collect(),
        )
    }
}
