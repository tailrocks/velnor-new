//! The Velnor-written `bunfig.toml` for `bun install`.
//!
//! The repository's `bunfig.toml` is never handed to the registry phase.
//! Velnor copies a closed set of install keys from it and appends the
//! private-scope registry itself, so the repository cannot redirect the
//! `packages: read` token to another host.

use toml::Value;
use toml::map::Map;

use crate::BunError;

/// The only registry a scope may point at.
pub const NPM_REGISTRY_URL: &str = "https://npm.pkg.github.com/";
/// Environment variable Bun expands for the scope token.
pub const NPM_TOKEN_ENV: &str = "VELNOR_NPM_READ_TOKEN";

/// `[install]` keys that redirect where Bun fetches from or whom it trusts.
const REDIRECTING: [&str; 5] = ["registry", "scopes", "cafile", "ca", "token"];
const LINKERS: [&str; 2] = ["isolated", "hoisted"];

/// Render the install configuration.
///
/// `repository` is the repository's `bunfig.toml` text, if any. Allowlisted
/// keys (`install.linker`, `install.exact`, `install.saveTextLockfile`) are
/// copied; every other key is dropped. With `with_token` the config also
/// maps each of `scopes` to the GitHub npm registry using
/// `$VELNOR_NPM_READ_TOKEN`; the lifecycle phase omits it.
///
/// # Errors
///
/// Returns `bunfig_invalid` for unparsable text, `bunfig_registry_override`
/// when the repository sets a registry, scope, token or certificate, and
/// `bunfig_value` for a copied key of the wrong type and `bunfig_scope` for a
/// scope that is not `@<name>`.
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

/// `@` plus lowercase alphanumerics, `-`, `_` and `.`: nothing TOML could re-parse.
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
