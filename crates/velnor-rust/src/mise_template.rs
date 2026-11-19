// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use tera::{Context, Tera};
use toml::Value;
use velnor_model::ToolRequest;

use crate::error::Reason;
use crate::mise_template_context::validate_statements;

#[derive(Deserialize)]
struct OrderedVars {
    #[serde(default)]
    vars: BTreeMap<String, toml::Spanned<Value>>,
}

pub(crate) fn render_versions(
    versions: &mut BTreeMap<String, Vec<ToolRequest>>,
    config: &toml::Table,
    bytes: &[u8],
) -> Result<(), Reason> {
    if !versions
        .values()
        .flatten()
        .any(|request| templated(&request.version))
    {
        return Ok(());
    }
    if config
        .get("settings")
        .and_then(Value::as_table)
        .and_then(|settings| settings.get("tera_v1"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Err(invalid("tera_v1 version templates are unsupported"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Reason::NotUtf8)?;
    let parsed: OrderedVars =
        toml::from_str(text).map_err(|error| Reason::Syntax(error.message().to_owned()))?;
    let mut vars: Vec<_> = parsed.vars.into_iter().collect();
    vars.sort_by_key(|(_, value)| value.span().start);
    let mut resolved = BTreeMap::new();
    for (name, value) in vars {
        if let Some(value) = var_value(value.into_inner())? {
            let value = render(&value, &context(&resolved))?;
            resolved.insert(name, value);
        } else {
            resolved.remove(&name);
        }
    }
    let context = context(&resolved);
    for request in versions.values_mut().flatten() {
        request.version = render(&request.version, &context)?;
    }
    Ok(())
}

fn context(vars: &BTreeMap<String, String>) -> Context {
    let mut context = Context::new();
    context.insert("vars", vars);
    context
}

fn var_value(value: Value) -> Result<Option<String>, Reason> {
    match value {
        Value::String(text) => Ok(Some(text)),
        Value::Integer(value) => Ok(Some(value.to_string())),
        Value::Boolean(true) => Ok(Some("true".to_owned())),
        Value::Boolean(false) => Ok(None),
        Value::Table(mut table) if table.len() == 1 && table.contains_key("value") => {
            match table.remove("value") {
                Some(Value::Table(_)) => {
                    Err(invalid("vars.value must be a string, integer or boolean"))
                }
                Some(value) => var_value(value),
                None => Err(invalid("vars.value is missing")),
            }
        }
        _ => Err(invalid(
            "vars must be strings, integers, booleans or a value table; other var directives are unsupported",
        )),
    }
}

fn render(text: &str, context: &Context) -> Result<String, Reason> {
    if !templated(text) {
        return Ok(text.to_owned());
    }
    validate_statements(text).map_err(invalid)?;
    let mut tera = Tera::default();
    tera.add_raw_template("version", text)
        .map_err(|error| invalid(&format!("unsupported version template: {error}")))?;
    for variable in tera
        .get_template_variables("version")
        .map_err(|error| invalid(&error.to_string()))?
    {
        if variable != "vars" {
            return Err(invalid(&format!(
                "unsupported template context: {variable}"
            )));
        }
    }
    let rendered = tera
        .render("version", context)
        .map_err(|error| invalid(&error.to_string()))?;
    if templated(&rendered) {
        return Err(invalid(&format!("unresolved template {text}")));
    }
    Ok(rendered)
}

fn templated(text: &str) -> bool {
    ["{{", "{%", "{#"]
        .iter()
        .any(|marker| text.contains(marker))
}

fn invalid(message: &str) -> Reason {
    Reason::InvalidValue {
        key: "tools.version template",
        value: format!(
            "{message}; version templates require deterministic config-local vars and Tera functions"
        ),
    }
}
