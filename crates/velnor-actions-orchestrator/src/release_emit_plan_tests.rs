// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ReleasePlan, check_creation_selection, release_triggers};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::config::{NewCrateRelease, RustReleaseConfig};
use velnor_actions_rust::release_select::{ReleaseRequest, ReleaseScope, select_release_set};

#[test]
fn bootstrap_scope_matches_every_expanded_workspace_package()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[workspace]\nmembers=['alpha','beta']\nresolver='3'\n",
    )?;
    for name in ["alpha", "beta"] {
        let path = temp.path().join(name);
        std::fs::create_dir_all(path.join("src"))?;
        std::fs::write(
            path.join("Cargo.toml"),
            format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n"),
        )?;
        std::fs::write(path.join("src/lib.rs"), "")?;
    }
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(temp.path())
        .output()?;
    assert!(output.status.success());
    let metadata = String::from_utf8(output.stdout)?;
    let root = temp.path().canonicalize()?;
    let empty = BTreeSet::new();
    let scope = ReleaseScope::PublishableWorkspace;
    let selected = select_release_set(&ReleaseRequest {
        metadata_json: &metadata,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &empty,
        known_manifests: &empty,
        supported_registries: &[],
    })?;
    assert_eq!(selected.len(), 2);
    let mut release = RustReleaseConfig {
        new_crates: Some(NewCrateRelease {
            source_sha: "a".repeat(40),
            packages: [("alpha".into(), "0.1.0".into())].into(),
        }),
        ..RustReleaseConfig::default()
    };
    assert!(check_creation_selection(&release, &selected).is_err());
    release
        .new_crates
        .as_mut()
        .ok_or("creation")?
        .packages
        .insert("beta".into(), "0.1.0".into());
    assert!(check_creation_selection(&release, &selected).is_ok());
    Ok(())
}

#[test]
fn first_publication_is_dispatch_only() {
    let plan = ReleasePlan {
        plan_id: "test".into(),
        repository: "owner/repo".into(),
        source_sha: "a".repeat(40),
        registry: "crates-io".into(),
        packages: BTreeMap::new(),
        version: None,
        dispatch: true,
    };
    let creation = release_triggers("main", &plan, true);
    assert!(creation.push_branches.is_empty());
    assert!(!creation.dispatch_inputs.is_empty());
    let recovery = release_triggers("main", &plan, false);
    assert_eq!(recovery.push_branches, ["main"]);
}
