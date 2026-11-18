// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

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
        "for manifest in Cargo.toml; do",
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
