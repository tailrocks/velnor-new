// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_render::rendered_pins;

const POLICY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../.velnor/version-policy.toml"
);
const RENDER_SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../velnor-render/src");

#[derive(Debug)]
struct PolicyAction {
    name: String,
    version: String,
    sha: String,
    eligible_on: Option<String>,
}

fn policy_actions() -> Result<Vec<PolicyAction>, Box<dyn Error>> {
    let text = std::fs::read_to_string(POLICY)?;
    let doc: toml::Table = toml::from_str(&text)?;
    let entries = doc
        .get("actions")
        .and_then(toml::Value::as_array)
        .ok_or("version-policy.toml has no [[actions]] entries")?;
    let mut actions = Vec::new();
    for entry in entries {
        let table = entry.as_table().ok_or("[[actions]] entry is not a table")?;
        let field = |key: &str| {
            table
                .get(key)
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        };
        actions.push(PolicyAction {
            name: field("name").ok_or("[[actions]] entry has no name")?,
            version: field("version").ok_or("[[actions]] entry has no version")?,
            sha: field("sha").ok_or("[[actions]] entry has no sha")?,
            eligible_on: field("eligible_on"),
        });
    }
    Ok(actions)
}

const MISE_VERSION: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.mise-version");
const MISE_TOML: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mise.toml");

fn toml_string(path: &str, key: &str) -> Result<String, Box<dyn Error>> {
    let doc: toml::Table = toml::from_str(&std::fs::read_to_string(path)?)?;
    let value = doc.get(key).and_then(toml::Value::as_str);
    Ok(value.ok_or("the file has no such string key")?.to_owned())
}

#[test]
fn mise_version_file_matches_the_version_policy_and_the_minimum_mise_toml_requires()
-> Result<(), Box<dyn Error>> {
    let file = std::fs::read_to_string(MISE_VERSION)?.trim().to_owned();
    let minimum = toml_string(MISE_TOML, "min_version")?;
    let policy: toml::Table = toml::from_str(&std::fs::read_to_string(POLICY)?)?;
    let pinned = policy
        .get("tools")
        .and_then(toml::Value::as_table)
        .and_then(|tools| tools.get("mise"))
        .and_then(toml::Value::as_str)
        .ok_or("version-policy.toml has no [tools] mise pin")?;
    assert_eq!(
        file, minimum,
        ".mise-version differs from mise.toml min_version"
    );
    assert_eq!(
        file, pinned,
        ".mise-version differs from version-policy.toml"
    );
    Ok(())
}

/// Every 40-character hex word in the source. A commit pin is always a full SHA.
fn commit_literals(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_ascii_hexdigit())
        .filter(|word| word.len() == 40)
}

#[test]
fn rendered_action_pins_match_the_version_policy() -> Result<(), Box<dyn Error>> {
    let policy = policy_actions()?;
    let mut problems = Vec::new();
    for pin in rendered_pins() {
        let Some(entry) = policy.iter().find(|entry| entry.name == pin.action) else {
            problems.push(format!(
                "{}@{}: no [[actions]] entry in .velnor/version-policy.toml",
                pin.action, pin.commit
            ));
            continue;
        };
        if entry.sha != pin.commit {
            problems.push(format!(
                "{}: the renderer emits {} but the policy records {} ({})",
                pin.action, pin.commit, entry.sha, entry.version
            ));
        }
        if let Some(tag) = pin.tag
            && tag != entry.version
        {
            problems.push(format!(
                "{}: the renderer records {tag} but the policy records {} ({})",
                pin.action, entry.version, entry.sha
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "rendered action pins disagree with .velnor/version-policy.toml:\n{}",
        problems.join("\n")
    );
    Ok(())
}

#[test]
fn every_commit_literal_in_the_renderer_is_a_listed_pin() -> Result<(), Box<dyn Error>> {
    let listed: BTreeSet<&str> = rendered_pins().iter().map(|pin| pin.commit).collect();
    let mut unlisted = BTreeSet::new();
    for entry in std::fs::read_dir(RENDER_SRC)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        for literal in commit_literals(&text) {
            if !listed.contains(literal) {
                let file = path.file_name().unwrap_or_default().to_string_lossy();
                unlisted.insert(format!("{file}: {literal}"));
            }
        }
    }
    assert!(
        unlisted.is_empty(),
        "commit literals in velnor-render that rendered_pins() does not list \
         (add them to crates/velnor-render/src/rendered_pins.rs):\n{}",
        unlisted.into_iter().collect::<Vec<_>>().join("\n")
    );
    Ok(())
}

#[test]
fn version_policy_entries_are_unique_full_shas_with_iso_eligible_dates()
-> Result<(), Box<dyn Error>> {
    let policy = policy_actions()?;
    let names: BTreeSet<&str> = policy.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names.len(), policy.len(), "duplicate [[actions]] name");
    for entry in &policy {
        assert!(
            entry.sha.len() == 40 && entry.sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "{} sha is not a full commit: {}",
            entry.name,
            entry.sha
        );
        if let Some(date) = &entry.eligible_on {
            let shape = date.len() == 10
                && date.bytes().enumerate().all(|(i, b)| match i {
                    4 | 7 => b == b'-',
                    _ => b.is_ascii_digit(),
                });
            assert!(
                shape,
                "{} eligible_on must be YYYY-MM-DD: {date}",
                entry.name
            );
        }
    }
    Ok(())
}
