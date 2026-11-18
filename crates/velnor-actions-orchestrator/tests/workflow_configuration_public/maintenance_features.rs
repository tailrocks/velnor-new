// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;

use super::workflow_configuration_fixture;

pub(super) fn owner_feature_repo(
    policy: WorkflowPolicy,
    tagged: bool,
    configuration_name: &str,
    feature: &str,
) -> Result<TempDir, Box<dyn Error>> {
    let policy_name = match policy {
        WorkflowPolicy::ConsumerV1 => "consumer-v1",
        WorkflowPolicy::VelnorRepositoryV1 => "velnor-repository-v1",
    };
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"{policy_name}\"\ndefault_branch = \"testmain\"\n\n[[stacks.rust.configurations]]\nname = \"{configuration_name}\"\nfeatures = [\"{feature}\"]\nall_features = false\ntarget = \"host\"\n"
    );
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        workflow_configuration_fixture::git(
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/tailrocks/velnor-new.git",
            ],
            root,
        )?;
        fs::write(
            root.join(".velnor/generator.lock"),
            include_str!("../../../../.velnor/generator.lock"),
        )?;
        fs::write(
            root.join(".velnor/controller-approvals.json"),
            include_str!("../../../../.velnor/controller-approvals.json"),
        )?;
        workflow_configuration_fixture::write_root_cargo_lock(root, &["demo", "maintenance"])?;
    }
    let mut root_manifest = fs::read_to_string(root.join("Cargo.toml"))?;
    root_manifest.push_str("\n[workspace]\nmembers = [\"crates/maintenance\"]\nresolver = \"2\"\n");
    fs::write(root.join("Cargo.toml"), root_manifest)?;
    write_owner_member(root, tagged)?;
    workflow_configuration_fixture::write_nextest_task(root)?;
    workflow_configuration_fixture::write_nextest_task(&root.join("crates/maintenance"))?;
    Ok(repo)
}

fn write_owner_member(root: &Path, tagged: bool) -> Result<(), Box<dyn Error>> {
    let owner_metadata = if tagged {
        "\n[package.metadata.velnor]\nv1-task-owner = \"repository-maintenance\"\n"
    } else {
        ""
    };
    let manifest = format!(
        "[package]\nname = \"maintenance\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{owner_metadata}\n[features]\nmaintenance-only = []\n"
    );
    let package_root = root.join("crates/maintenance");
    fs::create_dir_all(package_root.join("src"))?;
    fs::write(package_root.join("Cargo.toml"), manifest)?;
    fs::write(package_root.join("src/lib.rs"), "pub fn maintenance() {}\n")?;
    Ok(())
}
