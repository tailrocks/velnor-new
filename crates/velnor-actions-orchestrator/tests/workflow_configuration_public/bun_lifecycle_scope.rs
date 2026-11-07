// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use toml::Value;
use velnor_actions_bun::{NPM_REGISTRY_URL, NPM_TOKEN_ENV, velnor_bunfig};

const REPOSITORY: &str = "[install]\nlinker = \"isolated\"\nexact = true\n";

fn scopes() -> Vec<String> {
    vec!["@chainargos".to_owned(), "@fixture".to_owned()]
}

fn scope_table(
    rendered: &str,
    scope: &str,
) -> Result<Option<toml::map::Map<String, Value>>, Box<dyn Error>> {
    let parsed: toml::Table = rendered.parse()?;
    Ok(parsed
        .get("install")
        .and_then(|install| install.get("scopes"))
        .and_then(|scopes| scopes.get(scope))
        .and_then(Value::as_table)
        .cloned())
}

#[test]
fn registry_phase_config_points_each_scope_at_the_registry_with_the_token()
-> Result<(), Box<dyn Error>> {
    let rendered = velnor_bunfig(Some(REPOSITORY), &scopes(), true)?;
    for scope in scopes() {
        let table = scope_table(&rendered, &scope)?.ok_or("registry phase lost a scope")?;
        assert_eq!(
            table.get("url").and_then(Value::as_str),
            Some(NPM_REGISTRY_URL)
        );
        assert_eq!(
            table.get("token").and_then(Value::as_str),
            Some(format!("${NPM_TOKEN_ENV}").as_str())
        );
    }
    Ok(())
}

#[test]
fn lifecycle_phase_config_points_each_scope_at_the_registry_without_credentials()
-> Result<(), Box<dyn Error>> {
    let rendered = velnor_bunfig(Some(REPOSITORY), &scopes(), false)?;
    for scope in scopes() {
        let table = scope_table(&rendered, &scope)?.ok_or("lifecycle phase lost a scope")?;
        assert_eq!(
            table.get("url").and_then(Value::as_str),
            Some(NPM_REGISTRY_URL)
        );
        assert!(
            table.get("token").is_none(),
            "lifecycle scope carries a token"
        );
    }
    assert!(
        !rendered.contains(NPM_TOKEN_ENV),
        "lifecycle config names the token variable"
    );
    Ok(())
}

#[test]
fn phases_without_scopes_render_the_same_repository_install_table() -> Result<(), Box<dyn Error>> {
    let with_token = velnor_bunfig(Some(REPOSITORY), &[], true)?;
    let without_token = velnor_bunfig(Some(REPOSITORY), &[], false)?;
    assert_eq!(with_token, without_token);
    assert_eq!(
        with_token,
        "[install]\nlinker = \"isolated\"\nexact = true\n"
    );
    Ok(())
}

#[test]
fn lifecycle_phase_still_rejects_an_invalid_scope() {
    assert!(velnor_bunfig(None, &["chainargos".to_owned()], false).is_err());
}
