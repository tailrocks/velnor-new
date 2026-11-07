// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::non_inputs_release::{CONFIG, QUALIFIED_RUST};
use super::scoped_workspace::put;
use super::workflow_configuration_fixture::{git, make_repo};
use std::error::Error;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

#[test]
fn git_tag_configuration_preserves_selected_version_groups() -> Result<(), Box<dyn Error>> {
    for (groups, accepted) in [
        ("", true),
        (r#"version_groups = { shared = ["demo", "second"] }"#, true),
        (r#"version_groups = { shared = ["absent"] }"#, false),
        (r#"publish_features = { demo = ["extra"] }"#, false),
    ] {
        let rendered = git_tag_tree(groups);
        if !accepted {
            assert!(rendered.is_err(), "accepted {groups}");
            continue;
        }
        let tree = rendered?;
        let config: toml::Value = toml::from_str(
            tree.get(".github/release-plz.toml")
                .ok_or("missing release config")?,
        )?;
        let workspace = config.get("workspace").ok_or("missing workspace")?;
        for (key, expected) in [
            ("publish", false),
            ("git_only", true),
            ("semver_check", false),
        ] {
            assert_eq!(
                workspace.get(key).and_then(toml::Value::as_bool),
                Some(expected),
                "{key}"
            );
        }
        let packages = config
            .get("package")
            .and_then(toml::Value::as_array)
            .ok_or("missing packages")?;
        assert_eq!(packages.len(), 2);
        for (package, name) in packages.iter().zip(["demo", "second"]) {
            assert_eq!(
                package.get("name").and_then(toml::Value::as_str),
                Some(name)
            );
            assert_eq!(
                package.get("version_group").and_then(toml::Value::as_str),
                if groups.is_empty() {
                    None
                } else {
                    Some("shared")
                }
            );
        }
    }
    Ok(())
}

fn git_tag_tree(
    groups: &str,
) -> Result<velnor_actions_workflow_renderer::render::RenderedTree, Box<dyn Error>> {
    let prefix = CONFIG
        .split("[stacks.rust.binary_release]")
        .next()
        .ok_or("config prefix")?;
    let config = format!(
        r#"{prefix}
[stacks.rust.release]
enabled = true
registry = "git-tag"
packages = ["demo", "second"]
{groups}
{QUALIFIED_RUST}"#
    );
    let repo = make_repo(&config)?;
    let root = repo.path();
    put(
        root,
        "Cargo.toml",
        r#"[workspace]
members = ["crates/demo", "crates/second"]
resolver = "2"
"#,
    )?;
    for name in ["demo", "second"] {
        put(
            root,
            &format!("crates/{name}/Cargo.toml"),
            &format!(
                r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
[features]
extra = []
"#
            ),
        )?;
        put(
            root,
            &format!("crates/{name}/src/lib.rs"),
            "pub fn value() {}\n",
        )?;
    }
    put(
        root,
        "Cargo.lock",
        r#"version = 4
[[package]]
name = "demo"
version = "0.1.0"
[[package]]
name = "second"
version = "0.1.0"
"#,
    )?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/probe.git",
        ],
        root,
    )?;
    git(&["add", "-A"], root)?;
    git(&["commit", "-m", "grouped release fixture"], root)?;
    Ok(render_staged_tree(&prepare(root)?)?)
}
