// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use toml::Value;
use toml::map::Map;

use crate::BunError;

pub const NPM_REGISTRY_URL: &str = "https://npm.pkg.github.com/";

pub const NPM_TOKEN_ENV: &str = "VELNOR_NPM_READ_TOKEN";

const REDIRECTING: [&str; 5] = ["registry", "scopes", "cafile", "ca", "token"];
const LINKERS: [&str; 2] = ["isolated", "hoisted"];

pub fn velnor_bunfig(
    repository: Option<&str>,
    scopes: &[String],
    with_token: bool,
) -> Result<String, BunError> {
    let table = match repository {
        Some(text) => text
            .parse::<Map<String, Value>>()
            .map_err(|_| BunError::bare("bunfig_invalid"))?,
        None => Map::new(),
    };
    let install = table.get("install").and_then(Value::as_table);
    if let Some(install) = install
        && REDIRECTING.iter().any(|key| install.contains_key(*key))
    {
        return Err(BunError::bare("bunfig_registry_override"));
    }
    let mut lines = vec!["[install]".to_owned()];
    if let Some(install) = install {
        lines.extend(install_lines(install)?);
    }
    if with_token {
        for scope in scopes {
            if !is_scope(scope) {
                return Err(BunError::new("bunfig_scope", scope));
            }
            lines.push(String::new());
            lines.push(format!("[install.scopes.\"{scope}\"]"));
            lines.push(format!("url = \"{NPM_REGISTRY_URL}\""));
            lines.push(format!("token = \"${NPM_TOKEN_ENV}\""));
        }
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn is_scope(scope: &str) -> bool {
    scope.strip_prefix('@').is_some_and(|name| {
        !name.is_empty()
            && name.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
            })
    })
}

fn install_lines(install: &Map<String, Value>) -> Result<Vec<String>, BunError> {
    let mut lines = Vec::new();
    if let Some(linker) = install.get("linker") {
        let linker = linker
            .as_str()
            .filter(|linker| LINKERS.contains(linker))
            .ok_or_else(|| BunError::new("bunfig_value", "install.linker"))?;
        lines.push(format!("linker = \"{linker}\""));
    }
    for key in ["exact", "saveTextLockfile"] {
        if let Some(value) = install.get(key) {
            let flag = value
                .as_bool()
                .ok_or_else(|| BunError::new("bunfig_value", &format!("install.{key}")))?;
            lines.push(format!("{key} = {flag}"));
        }
    }
    Ok(lines)
}
