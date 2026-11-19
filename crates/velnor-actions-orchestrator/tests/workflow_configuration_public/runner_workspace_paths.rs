// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::workflow_configuration_fixture;

const DEFAULT_ROOT: &str = "crates/velnor-runner";

fn config(runner_workspace: Option<&str>) -> String {
    let mut text = "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nworkflows = [\"generator_release\", \"macos_binary_release\", \"image_release\", \"qualification\"]\n\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n".to_owned();
    if let Some(root) = runner_workspace {
        text = format!("{text}\n[runner_workspace]\nroot = \"{root}\"\n");
    }
    text
}

fn rendered(runner_workspace: Option<&str>) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(&config(runner_workspace))?;
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    let prepared = prepare(repo.path())?;
    Ok(render_staged_tree(&prepared)?
        .files
        .into_iter()
        .map(|file| (file.path, file.bytes))
        .collect())
}

fn count(tree: &BTreeMap<String, String>, needle: &str) -> usize {
    tree.values().map(|text| text.matches(needle).count()).sum()
}

#[test]
fn unset_runner_workspace_renders_todays_paths() -> Result<(), Box<dyn Error>> {
    let unset = rendered(None)?;
    assert_eq!(unset, rendered(Some(DEFAULT_ROOT))?);
    for needle in [
        "--manifest-path crates/velnor-runner/Cargo.toml",
        "crates/velnor-runner/target/release/velnor-host",
        "crates/velnor-runner/Cargo.lock",
        "for manifest in Cargo.toml crates/velnor-runner/Cargo.toml; do",
        "fetch_mode=crates/velnor-runner/Cargo.toml:",
        "CARGO_TARGET_DIR:-crates/velnor-runner/target",
    ] {
        assert!(count(&unset, needle) > 0, "default render lost {needle}");
    }
    Ok(())
}

#[test]
fn configured_runner_workspace_replaces_every_runner_path() -> Result<(), Box<dyn Error>> {
    let tree = rendered(Some("crates/velnor-rig"))?;
    assert_eq!(count(&tree, DEFAULT_ROOT), 0);
    for needle in [
        "--manifest-path crates/velnor-rig/Cargo.toml",
        "crates/velnor-rig/target/release/velnor-host",
        "crates/velnor-rig/Cargo.lock",
        "for manifest in Cargo.toml crates/velnor-rig/Cargo.toml; do",
        "fetch_mode=crates/velnor-rig/Cargo.toml:",
        "CARGO_TARGET_DIR:-crates/velnor-rig/target",
    ] {
        assert!(count(&tree, needle) > 0, "configured render lost {needle}");
    }
    Ok(())
}

#[test]
fn repository_root_runner_workspace_uses_the_root_manifest() -> Result<(), Box<dyn Error>> {
    let tree = rendered(Some(""))?;
    assert_eq!(count(&tree, DEFAULT_ROOT), 0);
    assert_eq!(count(&tree, "for manifest in Cargo.toml Cargo.toml"), 0);
    for needle in [
        "--manifest-path Cargo.toml",
        "--manifest-path \\\"$GITHUB_WORKSPACE/Cargo.toml\\\" > /dev/null",
        "target/release/velnor-host",
    ] {
        assert!(count(&tree, needle) > 0, "root render lost {needle}");
    }
    Ok(())
}

#[test]
fn malformed_runner_workspace_roots_are_rejected() -> Result<(), Box<dyn Error>> {
    for root in ["/abs", "crates/x/", "crates/../x", "a//b", "a b", "./x"] {
        let repo = workflow_configuration_fixture::make_repo(&config(Some(root)))?;
        assert!(prepare(repo.path()).is_err(), "accepted {root}");
    }
    Ok(())
}

const QUALIFICATION_MARKER: &str = "prefetch-locked-sources-for-qualification-workspaces";

fn collect_scripts(value: &serde_json::Value, found: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text)
            if text.contains(QUALIFICATION_MARKER) && text.contains("cargo metadata --locked") =>
        {
            found.push(text.clone());
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_scripts(item, found);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_scripts(item, found);
            }
        }
        _ => {}
    }
}

fn qualification_scripts(tree: &BTreeMap<String, String>) -> Result<Vec<String>, Box<dyn Error>> {
    let mut found = Vec::new();
    for (path, text) in tree {
        if std::path::Path::new(path)
            .extension()
            .is_some_and(|ext| ext == "yml")
            && text.contains(QUALIFICATION_MARKER)
        {
            let parsed: serde_json::Value = serde_saphyr::from_str(text)?;
            collect_scripts(&parsed, &mut found);
        }
    }
    assert!(!found.is_empty(), "no qualification script rendered");
    Ok(found)
}

fn shellcheck_required() -> bool {
    ["CI", "GITHUB_ACTIONS"]
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| value == "true"))
}

fn shellcheck(script: &str) -> Result<Option<String>, Box<dyn Error>> {
    let spawned = Command::new("shellcheck")
        .args(["--shell=bash", "--severity=style", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if shellcheck_required() {
                return Err(
                    "shellcheck is required in CI; install it via mise (mise.toml pins shellcheck)"
                        .into(),
                );
            }
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    child
        .stdin
        .take()
        .ok_or("shellcheck stdin")?
        .write_all(script.as_bytes())?;
    let output = child.wait_with_output()?;
    if output.status.success() {
        return Ok(Some(String::new()));
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    ))
}

fn assert_shellcheck_clean(tree: &BTreeMap<String, String>) -> Result<(), Box<dyn Error>> {
    for script in qualification_scripts(tree)? {
        if let Some(report) = shellcheck(&script)? {
            assert!(
                report.is_empty(),
                "shellcheck rejected the script:\n{report}"
            );
        }
    }
    Ok(())
}

#[test]
fn repository_root_runner_workspace_renders_no_single_item_loop() -> Result<(), Box<dyn Error>> {
    let tree = rendered(Some(""))?;
    for script in qualification_scripts(&tree)? {
        assert!(
            !script.contains("for manifest in"),
            "single-item loop: {script}"
        );
        assert!(script.contains("--manifest-path \"$GITHUB_WORKSPACE/Cargo.toml\""));
    }
    assert_shellcheck_clean(&tree)
}

#[test]
fn several_manifests_keep_the_loop_and_pass_shellcheck() -> Result<(), Box<dyn Error>> {
    let tree = rendered(Some("crates/velnor-rig"))?;
    for script in qualification_scripts(&tree)? {
        assert!(script.contains("for manifest in Cargo.toml crates/velnor-rig/Cargo.toml; do"));
        assert!(script.contains("--manifest-path \"$GITHUB_WORKSPACE/$manifest\""));
    }
    assert_shellcheck_clean(&tree)
}
