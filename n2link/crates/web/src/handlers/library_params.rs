//! Typed parameters of Flow Library templates, for the Copilot's template mode.
//!
//! A template may declare `params` (name → spec). Its draft then carries `{{param:<name>}}`
//! placeholders inside string values. In template mode the model only chooses a template and
//! fills these values; n2link checks them here, substitutes them and runs the normal draft
//! checks. Values are limited to plain characters (no quotes, `$`, braces, backslashes or
//! control characters), so a template may place them inside JSONata or SQL text.
//!
//! Kinds: `decimal` (string, `min`/`max`), `int` (`min`/`max`), `enum` (`values`),
//! `text` (`maxLen`, letters, digits, space, `.`, `_`, `-`) and `list` (`maxItems` lowercase words).
//! Every spec has a `default` and may have a `label`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

pub const MAX_PARAMS: usize = 8;
const MAX_PARAM_NAME: usize = 32;
const MAX_LABEL: usize = 80;
const MAX_TEXT: usize = 64;
const MAX_ENUM_VALUES: usize = 16;
const MAX_LIST_ITEMS: usize = 8;
const MAX_WORD: usize = 20;
const OPEN: &str = "{{param:";
const CLOSE: &str = "}}";
const DECIMAL_PATTERN: &str = "^[0-9]{1,9}(\\.[0-9]{1,6})?$";
const TEXT_PATTERN: &str = "^[A-Za-z0-9 ._-]+$";
const WORD_PATTERN: &str = "^[a-z][a-z0-9_]{0,19}$";

#[derive(Debug, Clone, PartialEq)]
pub enum ParamKind {
    Decimal { min: String, max: String },
    Int { min: i64, max: i64 },
    Enum { values: Vec<String> },
    Text { max_len: usize },
    List { max_items: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamSpec {
    pub kind: ParamKind,
    pub default: Value,
    pub label: String,
}

pub type ParamSpecs = BTreeMap<String, ParamSpec>;

/// Parse and check a template's `params` object. A missing or empty object means no parameters.
pub fn parse_specs(value: Option<&Value>) -> Result<ParamSpecs, String> {
    let Some(value) = value else { return Ok(ParamSpecs::new()) };
    let object = value.as_object().ok_or("params must be an object")?;
    if object.len() > MAX_PARAMS {
        return Err(format!("a template may declare at most {MAX_PARAMS} params"));
    }
    let mut specs = ParamSpecs::new();
    for (name, spec) in object {
        if !valid_param_name(name) {
            return Err(format!(
                "param name '{name}' must be lowercase letters, digits and '_' (max {MAX_PARAM_NAME})"
            ));
        }
        let spec = parse_spec(spec).map_err(|e| format!("param '{name}': {e}"))?;
        specs.insert(name.clone(), spec);
    }
    Ok(specs)
}

fn parse_spec(value: &Value) -> Result<ParamSpec, String> {
    let object = value.as_object().ok_or("spec must be an object")?;
    let kind_name = object.get("kind").and_then(Value::as_str).ok_or("spec needs a kind")?;
    let allowed: &[&str] = match kind_name {
        "decimal" | "int" => &["kind", "default", "label", "min", "max"],
        "enum" => &["kind", "default", "label", "values"],
        "text" => &["kind", "default", "label", "maxLen"],
        "list" => &["kind", "default", "label", "maxItems"],
        other => return Err(format!("unknown kind '{other}'")),
    };
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("unknown field '{key}' for kind {kind_name}"));
    }
    let kind = match kind_name {
        "decimal" => {
            let bound = |key: &str| -> Result<String, String> {
                match object.get(key).and_then(Value::as_str) {
                    Some(text) if is_decimal(text) => Ok(text.to_owned()),
                    _ => Err(format!("{key} must be a decimal string")),
                }
            };
            let (min, max) = (bound("min")?, bound("max")?);
            if decimal_value(&min) > decimal_value(&max) {
                return Err("min is above max".to_owned());
            }
            ParamKind::Decimal { min, max }
        }
        "int" => {
            let min = object.get("min").and_then(Value::as_i64).ok_or("min must be an integer")?;
            let max = object.get("max").and_then(Value::as_i64).ok_or("max must be an integer")?;
            if min > max {
                return Err("min is above max".to_owned());
            }
            ParamKind::Int { min, max }
        }
        "enum" => {
            let values: Vec<String> = object
                .get("values")
                .and_then(Value::as_array)
                .ok_or("values must be a list")?
                .iter()
                .map(|v| v.as_str().filter(|s| is_text(s, MAX_TEXT)).map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or("values must be plain strings")?;
            if values.is_empty() || values.len() > MAX_ENUM_VALUES {
                return Err(format!("values must list 1 to {MAX_ENUM_VALUES} choices"));
            }
            ParamKind::Enum { values }
        }
        "text" => {
            let max_len = object.get("maxLen").and_then(Value::as_u64).unwrap_or(MAX_TEXT as u64) as usize;
            if !(1..=MAX_TEXT).contains(&max_len) {
                return Err(format!("maxLen must be 1 to {MAX_TEXT}"));
            }
            ParamKind::Text { max_len }
        }
        _ => {
            let max_items = object.get("maxItems").and_then(Value::as_u64).unwrap_or(MAX_LIST_ITEMS as u64) as usize;
            if !(1..=MAX_LIST_ITEMS).contains(&max_items) {
                return Err(format!("maxItems must be 1 to {MAX_LIST_ITEMS}"));
            }
            ParamKind::List { max_items }
        }
    };
    let label = match object.get("label") {
        None => String::new(),
        Some(Value::String(text)) if text.chars().count() <= MAX_LABEL && !text.chars().any(char::is_control) => {
            text.clone()
        }
        Some(_) => return Err(format!("label must be text of at most {MAX_LABEL} characters")),
    };
    let mut spec = ParamSpec { kind, default: Value::Null, label };
    let default = object.get("default").ok_or("spec needs a default")?;
    spec.default = check_value(&spec, default).map_err(|e| format!("default: {e}"))?;
    Ok(spec)
}

/// Check one value against its spec and return it in canonical form.
pub fn check_value(spec: &ParamSpec, value: &Value) -> Result<Value, String> {
    match &spec.kind {
        ParamKind::Decimal { min, max } => {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                _ => return Err("expected a decimal".to_owned()),
            };
            if !is_decimal(&text) {
                return Err("expected a decimal such as \"25\" or \"0.5\"".to_owned());
            }
            let number = decimal_value(&text);
            if number < decimal_value(min) || number > decimal_value(max) {
                return Err(format!("{text} is outside {min}..{max}"));
            }
            Ok(Value::String(text))
        }
        ParamKind::Int { min, max } => {
            let number = match value {
                Value::Number(number) => number.as_i64(),
                Value::String(text) if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) => {
                    text.parse().ok()
                }
                _ => None,
            }
            .ok_or("expected an integer")?;
            if number < *min || number > *max {
                return Err(format!("{number} is outside {min}..{max}"));
            }
            Ok(json!(number))
        }
        ParamKind::Enum { values } => match value.as_str() {
            Some(text) if values.iter().any(|v| v == text) => Ok(Value::String(text.to_owned())),
            _ => Err(format!("expected one of {}", values.join(", "))),
        },
        ParamKind::Text { max_len } => match value.as_str() {
            Some(text) if is_text(text, *max_len) => Ok(Value::String(text.to_owned())),
            _ => Err(format!("expected 1 to {max_len} letters, digits, spaces, '.', '_' or '-'")),
        },
        ParamKind::List { max_items } => {
            let items = value.as_array().ok_or("expected a list")?;
            if items.is_empty() || items.len() > *max_items {
                return Err(format!("expected 1 to {max_items} items"));
            }
            let mut seen = BTreeSet::new();
            for item in items {
                let word = item.as_str().filter(|w| is_word(w)).ok_or("items must be lowercase words")?;
                if !seen.insert(word) {
                    return Err(format!("'{word}' is listed twice"));
                }
            }
            Ok(value.clone())
        }
    }
}

/// The values to use: the model's values where given (checked), the defaults otherwise.
/// Also returns the names that fell back to their default. Unknown names are refused.
pub fn resolve_values(
    specs: &ParamSpecs,
    given: &Map<String, Value>,
) -> Result<(BTreeMap<String, Value>, Vec<String>), String> {
    if let Some(name) = given.keys().find(|name| !specs.contains_key(*name)) {
        return Err(format!("'{name}' is not a parameter of this template"));
    }
    let mut values = BTreeMap::new();
    let mut defaulted = Vec::new();
    for (name, spec) in specs {
        let value = match given.get(name) {
            Some(value) => check_value(spec, value).map_err(|e| format!("{name}: {e}"))?,
            None => {
                defaulted.push(name.clone());
                spec.default.clone()
            }
        };
        values.insert(name.clone(), value);
    }
    Ok((values, defaulted))
}

/// The defaults of every parameter.
pub fn defaults(specs: &ParamSpecs) -> BTreeMap<String, Value> {
    specs.iter().map(|(name, spec)| (name.clone(), spec.default.clone())).collect()
}

/// Every `{{param:<name>}}` used in string values of a draft.
pub fn placeholders(draft: &Value) -> Result<BTreeSet<String>, String> {
    let mut found = BTreeSet::new();
    let mut error = None;
    walk_strings(draft, &mut |text| {
        let mut rest = text;
        while let Some(start) = rest.find(OPEN) {
            let after = &rest[start + OPEN.len()..];
            match after.find(CLOSE) {
                Some(end) if valid_param_name(&after[..end]) => {
                    found.insert(after[..end].to_owned());
                    rest = &after[end + CLOSE.len()..];
                }
                _ => {
                    error.get_or_insert_with(|| format!("malformed placeholder in '{text}'"));
                    break;
                }
            }
        }
    });
    error.map_or(Ok(found), Err)
}

/// Declared params and used placeholders must match exactly, so a typo cannot slip through.
pub fn check_template(specs: &ParamSpecs, draft: &Value) -> Result<(), String> {
    let used = placeholders(draft)?;
    if let Some(name) = used.iter().find(|name| !specs.contains_key(*name)) {
        return Err(format!("placeholder {OPEN}{name}{CLOSE} is not declared in params"));
    }
    if let Some(name) = specs.keys().find(|name| !used.contains(*name)) {
        return Err(format!("param '{name}' is declared but never used in the draft"));
    }
    Ok(())
}

/// Replace every placeholder. A string that is exactly one integer placeholder becomes a number;
/// everywhere else the value is inserted as text (a list as its JSON text).
pub fn substitute(draft: &Value, values: &BTreeMap<String, Value>) -> Result<Value, String> {
    match draft {
        Value::String(text) => {
            if let Some(name) = text.strip_prefix(OPEN).and_then(|t| t.strip_suffix(CLOSE))
                && let Some(Value::Number(number)) = values.get(name)
            {
                return Ok(Value::Number(number.clone()));
            }
            let mut out = String::with_capacity(text.len());
            let mut rest = text.as_str();
            while let Some(start) = rest.find(OPEN) {
                out.push_str(&rest[..start]);
                let after = &rest[start + OPEN.len()..];
                let end = after.find(CLOSE).ok_or_else(|| format!("malformed placeholder in '{text}'"))?;
                let value = values.get(&after[..end]).ok_or_else(|| format!("no value for '{}'", &after[..end]))?;
                out.push_str(&render(value));
                rest = &after[end + CLOSE.len()..];
            }
            out.push_str(rest);
            Ok(Value::String(out))
        }
        Value::Array(items) => {
            items.iter().map(|item| substitute(item, values)).collect::<Result<_, _>>().map(Value::Array)
        }
        Value::Object(object) => object
            .iter()
            .map(|(key, value)| substitute(value, values).map(|value| (key.clone(), value)))
            .collect::<Result<Map<_, _>, _>>()
            .map(Value::Object),
        other => Ok(other.clone()),
    }
}

/// Specs as JSON, the form stored in the library and shown to the model.
pub fn specs_json(specs: &ParamSpecs) -> Value {
    let mut out = Map::new();
    for (name, spec) in specs {
        let mut entry = match &spec.kind {
            ParamKind::Decimal { min, max } => json!({"kind": "decimal", "min": min, "max": max}),
            ParamKind::Int { min, max } => json!({"kind": "int", "min": min, "max": max}),
            ParamKind::Enum { values } => json!({"kind": "enum", "values": values}),
            ParamKind::Text { max_len } => json!({"kind": "text", "maxLen": max_len}),
            ParamKind::List { max_items } => json!({"kind": "list", "maxItems": max_items}),
        };
        entry["default"] = spec.default.clone();
        if !spec.label.is_empty() {
            entry["label"] = json!(spec.label);
        }
        out.insert(name.clone(), entry);
    }
    Value::Object(out)
}

/// The JSON schema a template-mode reply must match: one branch per template, plus `none`.
pub fn reply_schema(templates: &[(&str, &ParamSpecs)]) -> Value {
    let object = |properties: Map<String, Value>| {
        let required: Vec<&String> = properties.keys().collect();
        json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
    };
    let branch = |name: &str, params: Value| {
        let mut properties = Map::new();
        properties.insert("version".to_owned(), json!({"const": 1}));
        properties.insert("template".to_owned(), json!({"const": name}));
        properties.insert("params".to_owned(), params);
        properties.insert("summary".to_owned(), json!({"type": "string", "maxLength": 300}));
        object(properties)
    };
    let mut branches: Vec<Value> = templates
        .iter()
        .map(|(name, specs)| {
            let params = specs.iter().map(|(param, spec)| (param.clone(), value_schema(&spec.kind))).collect();
            branch(name, object(params))
        })
        .collect();
    branches.push(branch("none", object(Map::new())));
    json!({"anyOf": branches})
}

fn value_schema(kind: &ParamKind) -> Value {
    match kind {
        ParamKind::Decimal { .. } => json!({"type": "string", "pattern": DECIMAL_PATTERN}),
        ParamKind::Int { min, max } => json!({"type": "integer", "minimum": min, "maximum": max}),
        ParamKind::Enum { values } => json!({"type": "string", "enum": values}),
        ParamKind::Text { max_len } => json!({"type": "string", "maxLength": max_len, "pattern": TEXT_PATTERN}),
        ParamKind::List { max_items } => json!({
            "type": "array", "minItems": 1, "maxItems": max_items,
            "items": {"type": "string", "pattern": WORD_PATTERN},
        }),
    }
}

/// `name=value` pairs for summaries, in name order.
pub fn describe(values: &BTreeMap<String, Value>) -> String {
    values.iter().map(|(name, value)| format!("{name}={}", render(value))).collect::<Vec<_>>().join(", ")
}

fn render(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn walk_strings(value: &Value, visit: &mut impl FnMut(&str)) {
    match value {
        Value::String(text) => visit(text),
        Value::Array(items) => items.iter().for_each(|item| walk_strings(item, visit)),
        Value::Object(object) => object.values().for_each(|item| walk_strings(item, visit)),
        _ => {}
    }
}

fn valid_param_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= MAX_PARAM_NAME
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_decimal(text: &str) -> bool {
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (text, None),
    };
    let digits =
        |part: &str, max: usize| !part.is_empty() && part.len() <= max && part.chars().all(|c| c.is_ascii_digit());
    digits(whole, 9) && fraction.is_none_or(|f| digits(f, 6))
}

fn decimal_value(text: &str) -> f64 {
    text.parse().unwrap_or(f64::NAN)
}

fn is_text(text: &str, max_len: usize) -> bool {
    let count = text.chars().count();
    (1..=max_len).contains(&count)
        && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'))
}

fn is_word(text: &str) -> bool {
    let mut chars = text.chars();
    text.len() <= MAX_WORD
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs() -> ParamSpecs {
        parse_specs(Some(&json!({
            "price": {"kind": "decimal", "min": "0.01", "max": "1000", "default": "5", "label": "Price"},
            "minutes": {"kind": "int", "min": 1, "max": 1440, "default": 15},
            "currency": {"kind": "enum", "values": ["USDT"], "default": "USDT"},
            "shop": {"kind": "text", "maxLen": 20, "default": "Demo shop"},
            "envelopes": {"kind": "list", "maxItems": 3, "default": ["food", "savings"]},
        })))
        .unwrap()
    }

    #[test]
    fn specs_parse_and_round_trip() {
        let specs = specs();
        assert_eq!(specs.len(), 5);
        assert_eq!(specs["minutes"].default, json!(15));
        let again = parse_specs(Some(&specs_json(&specs))).unwrap();
        assert_eq!(again, specs);
        assert!(parse_specs(None).unwrap().is_empty());
    }

    #[test]
    fn bad_specs_are_refused() {
        let bad = [
            json!({"Price": {"kind": "int", "min": 1, "max": 2, "default": 1}}),
            json!({"p": {"kind": "float", "default": 1}}),
            json!({"p": {"kind": "int", "min": 1, "max": 2, "default": 3}}),
            json!({"p": {"kind": "int", "min": 5, "max": 2, "default": 3}}),
            json!({"p": {"kind": "int", "min": 1, "max": 2, "default": 1, "step": 1}}),
            json!({"p": {"kind": "decimal", "min": "1", "max": "-2", "default": "1"}}),
            json!({"p": {"kind": "enum", "values": ["a'b"], "default": "a'b"}}),
            json!({"p": {"kind": "list", "maxItems": 2}}),
            json!(["not", "an", "object"]),
        ];
        for spec in bad {
            assert!(parse_specs(Some(&spec)).is_err(), "accepted {spec}");
        }
    }

    #[test]
    fn values_are_checked_and_canonical() {
        let specs = specs();
        let ok = Map::from_iter([
            ("price".to_owned(), json!(25)),
            ("minutes".to_owned(), json!("30")),
            ("envelopes".to_owned(), json!(["food", "transport", "savings"])),
        ]);
        let (values, defaulted) = resolve_values(&specs, &ok).unwrap();
        assert_eq!(values["price"], json!("25"));
        assert_eq!(values["minutes"], json!(30));
        assert_eq!(defaulted, vec!["currency".to_owned(), "shop".to_owned()]);

        let refused = [
            ("price", json!("0.001")),
            ("price", json!("5000")),
            ("price", json!("1e3")),
            ("minutes", json!(0)),
            ("minutes", json!(2.5)),
            ("currency", json!("BTC")),
            ("shop", json!("Joe's")),
            ("shop", json!("a$b")),
            ("shop", json!("{{param:x}}")),
            ("shop", json!("x\ny")),
            ("envelopes", json!(["Food"])),
            ("envelopes", json!(["a", "a"])),
            ("envelopes", json!(["a", "b", "c", "d"])),
            ("unknown", json!("x")),
        ];
        for (name, value) in refused {
            let given = Map::from_iter([(name.to_owned(), value.clone())]);
            assert!(resolve_values(&specs, &given).is_err(), "accepted {name}={value}");
        }
    }

    #[test]
    fn placeholders_must_match_declared_params() {
        let specs =
            parse_specs(Some(&json!({"price": {"kind": "decimal", "min": "1", "max": "9", "default": "5"}}))).unwrap();
        let draft = json!({"nodes": [{"config": {"rules": [{"to": "{{param:price}}"}]}}]});
        assert!(check_template(&specs, &draft).is_ok());
        let typo = json!({"nodes": [{"config": {"to": "{{param:prise}}"}}]});
        assert!(check_template(&specs, &typo).unwrap_err().contains("not declared"));
        assert!(check_template(&specs, &json!({"nodes": []})).unwrap_err().contains("never used"));
        assert!(placeholders(&json!("{{param:price")).is_err());
        assert!(placeholders(&json!("{{param:Bad}}")).is_err());
    }

    #[test]
    fn substitution_types_and_text() {
        let values = BTreeMap::from([
            ("minutes".to_owned(), json!(15)),
            ("price".to_owned(), json!("5")),
            ("envelopes".to_owned(), json!(["food", "savings"])),
        ]);
        let draft = json!({
            "port": "{{param:minutes}}",
            "sql": "select {{param:price}} as price, '{{param:minutes}} min' as label",
            "list": "{{param:envelopes}}",
            "keep": 3,
        });
        let out = substitute(&draft, &values).unwrap();
        assert_eq!(out["port"], json!(15));
        assert_eq!(out["sql"], json!("select 5 as price, '15 min' as label"));
        assert_eq!(out["list"], json!("[\"food\",\"savings\"]"));
        assert_eq!(out["keep"], json!(3));
        assert!(substitute(&json!("{{param:other}}"), &values).is_err());
        assert!(placeholders(&out).unwrap().is_empty());
    }

    #[test]
    fn reply_schema_lists_every_template_and_none() {
        let specs = specs();
        let empty = ParamSpecs::new();
        let schema = reply_schema(&[("Shop", &specs), ("Plain", &empty)]);
        let branches = schema["anyOf"].as_array().unwrap();
        assert_eq!(branches.len(), 3);
        let names: Vec<&Value> = branches.iter().map(|b| &b["properties"]["template"]["const"]).collect();
        assert_eq!(names, vec![&json!("Shop"), &json!("Plain"), &json!("none")]);
        let params = &branches[0]["properties"]["params"];
        assert_eq!(params["required"].as_array().unwrap().len(), 5);
        assert_eq!(params["properties"]["minutes"]["maximum"], json!(1440));
        assert_eq!(params["additionalProperties"], json!(false));
        assert_eq!(
            describe(&defaults(&specs)),
            "currency=USDT, envelopes=[\"food\",\"savings\"], minutes=15, price=5, shop=Demo shop"
        );
    }
}
