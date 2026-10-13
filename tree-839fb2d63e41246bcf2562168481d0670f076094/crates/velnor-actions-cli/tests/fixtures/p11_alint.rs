//! P11 extended alint policy: bundle pins, zero same-id overrides.
//!
//! The frozen P00 snapshot pins the seven local generic rules; this module
//! pins everything around them: the exact deduped `extends` bundle set and
//! the absence of same-id overrides. The old `gha-pin-actions-to-sha`
//! demotion (reviewed-tag exception for a tag-pinned action) is gone:
//! every action ref is a full SHA, so bundled rules govern at default
//! severity. Any future exception must update this pin deliberately.

use std::error::Error;

/// The five pr-1 R04 bundles; the review example repeats the GHA entry.
pub(crate) const EXTENDS: [&str; 5] = [
    "alint://bundled/oss-baseline@v1",
    "alint://bundled/ci/github-actions@v1",
    "alint://bundled/rust@v1",
    "alint://bundled/hygiene/lockfiles@v1",
    "alint://bundled/hygiene/no-tracked-artifacts@v1",
];

/// Strip one matching single/double quote pair.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    for quote in ['\'', '"'] {
        if let Some(inner) = trimmed
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.to_owned();
        }
    }
    trimmed.to_owned()
}

/// Top-level `extends` list items (indent-aware; comments rejected).
pub(crate) fn extends_of(text: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut items = Vec::new();
    let mut in_extends = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            in_extends = line.trim() == "extends:";
            continue;
        }
        if in_extends && indent == 2 {
            let item = line
                .trim()
                .strip_prefix("- ")
                .ok_or_else(|| format!("bad extends item: {line}"))?;
            items.push(unquote(item.trim()));
        }
    }
    Ok(items)
}

/// Assert the extends set is exactly the five deduped bundles.
pub(crate) fn check_extends(text: &str) -> Result<(), Box<dyn Error>> {
    let mut found = extends_of(text)?;
    found.sort_unstable();
    let mut want = EXTENDS.to_vec();
    want.sort_unstable();
    if found != want {
        return Err(format!("extends is {found:?}, want {want:?}").into());
    }
    let mut deduped = found.clone();
    deduped.dedup();
    if deduped.len() != found.len() {
        return Err("extends repeats a bundle".into());
    }
    Ok(())
}

/// Full policy: frozen legacy seven plus the pinned single override.
pub(crate) fn check_extended_policy(text: &str) -> Result<(), Box<dyn Error>> {
    assert_eq!(super::alint_miniyaml::ALLOWED_KINDS.len(), 6);
    assert!(super::alint_miniyaml::ALLOWED_KINDS.contains(&"pair"));
    check_extends(text)?;
    let config = super::alint_miniyaml::parse(text)?;
    let mut legacy = Vec::new();
    let mut extra = Vec::new();
    for rule in config.rules {
        let known = super::alint_miniyaml::EXPECTED
            .iter()
            .any(|row| row.id == rule.id.as_str());
        if known {
            legacy.push(rule);
        } else {
            extra.push(rule);
        }
    }
    super::alint_miniyaml::check_policy(&super::alint_miniyaml::AlintConfig {
        version: config.version,
        rules: legacy,
    })?;
    if !extra.is_empty() {
        let ids: Vec<&str> = extra.iter().map(|rule| rule.id.as_str()).collect();
        return Err(format!("same-id overrides are banned, saw {ids:?}").into());
    }
    Ok(())
}

#[test]
fn alint_extended_policy_holds() -> Result<(), Box<dyn Error>> {
    check_extended_policy(&super::read(".alint.yml")?)
}

#[test]
fn alint_same_id_override_fails() -> Result<(), Box<dyn Error>> {
    let body = super::p11_toml::fixture("p11_alint_override_fail.yml")?;
    super::alint_miniyaml::parse(&body)?;
    assert!(
        check_extended_policy(&body).is_err(),
        "same-id override passed"
    );
    Ok(())
}

#[test]
fn alint_duped_extends_fails() -> Result<(), Box<dyn Error>> {
    let body = super::p11_toml::fixture("p11_alint_duped_extends_fail.yml")?;
    super::alint_miniyaml::parse(&body)?;
    assert!(
        check_extended_policy(&body).is_err(),
        "duped extends passed"
    );
    Ok(())
}
