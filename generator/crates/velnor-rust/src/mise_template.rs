// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use tera::{Context, Tera};
use toml::Value;
use velnor_model::ToolRequest;

use crate::error::Reason;

pub(crate) fn render_versions(
    versions: &mut BTreeMap<String, Vec<ToolRequest>>,
    config: &toml::Table,
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
    let mut pending = config
        .get("vars")
        .and_then(Value::as_table)
        .cloned()
        .unwrap_or_default();
    let mut resolved = toml::Table::new();
    while !pending.is_empty() {
        let mut progress = false;
        let mut failure = None;
        for (name, value) in pending.clone() {
            match render_value(value, &context(&resolved)) {
                Ok(value) => {
                    resolved.insert(name.clone(), value);
                    pending.remove(&name);
                    progress = true;
                }
                Err(error) => failure = Some(error),
            }
        }
        if !progress {
            return Err(failure.unwrap_or_else(|| invalid("unresolved vars")));
        }
    }
    let context = context(&resolved);
    for request in versions.values_mut().flatten() {
        request.version = render(&request.version, &context)?;
    }
    Ok(())
}

fn context(vars: &toml::Table) -> Context {
    let mut context = Context::new();
    context.insert("vars", vars);
    context
}

fn render_value(value: Value, context: &Context) -> Result<Value, Reason> {
    Ok(match value {
        Value::String(text) => Value::String(render(&text, context)?),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| render_value(value, context))
                .collect::<Result<_, _>>()?,
        ),
        Value::Table(table) => Value::Table(
            table
                .into_iter()
                .map(|(key, value)| render_value(value, context).map(|value| (key, value)))
                .collect::<Result<_, _>>()?,
        ),
        other => other,
    })
}

fn render(text: &str, context: &Context) -> Result<String, Reason> {
    if !templated(text) {
        return Ok(text.to_owned());
    }
    let rendered =
        Tera::one_off(text, context, false).map_err(|error| invalid(&error.to_string()))?;
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
