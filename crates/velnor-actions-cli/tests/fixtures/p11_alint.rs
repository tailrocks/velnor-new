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

const LOCKFILE_BUNDLE: &str = "alint://bundled/hygiene/lockfiles@v1";
const LOCKFILE_EXCEPT: [&str; 2] = ["lockfiles-no-nested-cargo", "lockfiles-no-nested-npm"];

/// One `extends` entry: bundle URL plus any `except` ids.
pub(crate) struct ExtendEntry {
    url: String,
    except: Vec<String>,
}

/// Top-level `extends` entries. A bare string or `url:` mapping; `except`
/// is the only nested key, and only on the lockfile bundle.
pub(crate) fn extends_of(text: &str) -> Result<Vec<ExtendEntry>, Box<dyn Error>> {
    let mut items = Vec::new();
    let mut in_extends = false;
    let mut excepting = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            in_extends = line.trim() == "extends:";
            excepting = false;
            continue;
        }
        if !in_extends {
            continue;
        }
        if indent == 2 {
            excepting = false;
            let item = line
                .trim()
                .strip_prefix("- ")
                .ok_or_else(|| format!("bad extends item: {line}"))?;
            let url = item
                .trim()
                .strip_prefix("url:")
                .map_or_else(|| item.trim(), str::trim);
            items.push(ExtendEntry {
                url: unquote(url),
                except: Vec::new(),
            });
            continue;
        }
        if indent == 4 && line.trim() == "except:" {
            excepting = true;
            continue;
        }
        if excepting && indent == 6 {
            let item = line
                .trim()
                .strip_prefix("- ")
                .ok_or_else(|| format!("bad except item: {line}"))?;
            items
                .last_mut()
                .ok_or("except without a bundle")?
                .except
                .push(unquote(item.trim()));
            continue;
        }
        return Err(format!("bad extends item: {line}").into());
    }
    Ok(items)
}

/// Assert the five bundles, no repeats, and the exact lockfile `except`.
pub(crate) fn check_extends(text: &str) -> Result<(), Box<dyn Error>> {
    let found = extends_of(text)?;
    let mut urls: Vec<String> = found.iter().map(|entry| entry.url.clone()).collect();
    urls.sort_unstable();
    let mut want = EXTENDS.to_vec();
    want.sort_unstable();
    if urls != want {
        return Err(format!("extends is {urls:?}, want {want:?}").into());
    }
    let mut deduped = urls.clone();
    deduped.dedup();
    if deduped.len() != urls.len() {
        return Err("extends repeats a bundle".into());
    }
    for entry in &found {
        let mut except = entry.except.clone();
        except.sort_unstable();
        if entry.url == LOCKFILE_BUNDLE {
            let want: Vec<&str> = LOCKFILE_EXCEPT.to_vec();
            let got: Vec<&str> = except.iter().map(String::as_str).collect();
            if got != want {
                return Err(format!("lockfile except is {got:?}").into());
            }
        } else if !except.is_empty() {
            return Err(format!("except on {}", entry.url).into());
        }
    }
    Ok(())
}

/// Narrow lockfile replacements. Same-id overrides of [`EXPECTED`] stay banned.
const NARROW_LOCKS: [super::alint_miniyaml::ExpectedRule; 2] = [
    super::alint_miniyaml::ExpectedRule {
        id: "velnor-no-stray-cargo-lock",
        kind: "file_absent",
        paths: &[
            ("paths.include", &["**/Cargo.lock"]),
            (
                "paths.exclude",
                &["Cargo.lock", "crates/velnor-runner/Cargo.lock"],
            ),
        ],
        pairs: &[],
    },
    super::alint_miniyaml::ExpectedRule {
        id: "velnor-no-stray-npm-lock",
        kind: "file_absent",
        paths: &[
            ("paths.include", &["**/package-lock.json"]),
            (
                "paths.exclude",
                &[
                    "package-lock.json",
                    "qualification/testcontainers/package-lock.json",
                ],
            ),
        ],
        pairs: &[],
    },
];

/// The two replacement ids, exact paths. Anything else is an unpinned rule.
fn check_narrow_locks(rules: &[super::alint_miniyaml::AlintRule]) -> Result<(), Box<dyn Error>> {
    let mut ids: Vec<&str> = rules.iter().map(|rule| rule.id.as_str()).collect();
    ids.sort_unstable();
    let mut want: Vec<&str> = NARROW_LOCKS.iter().map(|row| row.id).collect();
    want.sort_unstable();
    if ids != want {
        return Err(format!("unpinned rule ids {ids:?}, want {want:?}").into());
    }
    for rule in rules {
        super::alint_miniyaml::check_rule_shape(rule)?;
        let row = NARROW_LOCKS
            .iter()
            .find(|row| row.id == rule.id)
            .ok_or_else(|| format!("unknown rule {}", rule.id))?;
        if rule.kind() != row.kind {
            return Err(format!("{} kind is {}, want {}", rule.id, rule.kind(), row.kind).into());
        }
        for (key, values) in row.paths {
            let mut found: Vec<&str> = rule
                .pairs
                .iter()
                .filter(|pair| pair.0 == *key)
                .map(|pair| pair.1.as_str())
                .collect();
            found.sort_unstable();
            let mut expect = values.to_vec();
            expect.sort_unstable();
            if found != expect {
                return Err(format!("{} {key} is {found:?}, want {expect:?}", rule.id).into());
            }
        }
    }
    Ok(())
}

/// Full policy: frozen legacy seven plus the two narrow lockfile rules.
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
    check_narrow_locks(&extra)?;
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
