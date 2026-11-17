// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Deserialize;
use tera::{Context, Tera};
use toml::Value;
use velnor_model::ToolRequest;

use crate::error::Reason;

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
    let mut resolved = toml::Table::new();
    for (name, value) in vars {
        let value = render_value(value.into_inner(), &context(&resolved))?;
        resolved.insert(name, value);
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
