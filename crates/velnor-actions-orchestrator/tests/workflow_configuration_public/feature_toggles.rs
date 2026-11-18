// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;

use super::maintenance_features::owner_feature_repo;

pub(super) fn feature_toggle_repo(
    clippy_all_features: bool,
    all_features: bool,
) -> Result<TempDir, Box<dyn Error>> {
    let repo = owner_feature_repo(
        WorkflowPolicy::ConsumerV1,
        false,
        "feature-toggle",
        "default",
    )?;
    let root = repo.path();
    remove_feature_toggle_nextest_task(root)?;
    remove_feature_toggle_nextest_task(&root.join("crates/maintenance"))?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"maintenance\"\nversion = \"0.1.0\"\n",
    )?;
    let manifest_path = root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)?;
    let workspace = "\n[workspace]\nmembers = [\"crates/maintenance\"]\nresolver = \"2\"\n";
    assert_eq!(manifest.matches(workspace).count(), 1);
    let features = "\n[features]\ndefault = []\ndemo-only = []\n";
    let replacement = format!("{features}{workspace}");
    let manifest = manifest.replacen(workspace, &replacement, 1);
    assert!(manifest.contains("demo-only = []"));
    fs::write(manifest_path, manifest)?;
    let maintenance_manifest = root.join("crates/maintenance/Cargo.toml");
    let manifest = fs::read_to_string(&maintenance_manifest)?;
    let maintenance_features = "[features]\nmaintenance-only = []";
    assert_eq!(manifest.matches(maintenance_features).count(), 1);
    let manifest = manifest.replacen(
        maintenance_features,
        "[features]\ndefault = []\nmaintenance-only = []",
        1,
    );
    assert!(manifest.contains("default = []"));
    fs::write(maintenance_manifest, manifest)?;
    write_feature_toggle_config(root, clippy_all_features, all_features)?;
    Ok(repo)
}

fn remove_feature_toggle_nextest_task(root: &Path) -> Result<(), Box<dyn Error>> {
    let task = root.join(".mise/tasks/test");
    assert_eq!(
        fs::read_to_string(&task)?,
        "#!/bin/sh\ncargo nextest run --locked\n"
    );
    fs::remove_file(task)?;
    Ok(())
}

fn write_feature_toggle_config(
    root: &Path,
    clippy_all_features: bool,
    all_features: bool,
) -> Result<(), Box<dyn Error>> {
    let clippy_setting = if clippy_all_features {
        "clippy_all_features = true\n"
    } else {
        ""
    };
    let features = if all_features { "[]" } else { "[\"default\"]" };
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[stacks.rust]\n{clippy_setting}\n[[stacks.rust.configurations]]\nname = \"feature-toggle\"\nfeatures = {features}\nall_features = {all_features}\ntarget = \"host\"\n"
    );
    fs::write(root.join(".velnor/config.toml"), config)?;
    Ok(())
}
