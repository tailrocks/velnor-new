// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write as _;
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
    let default_branch = match policy {
        WorkflowPolicy::ConsumerV1 => "testmain",
        WorkflowPolicy::VelnorRepositoryV1 => "main",
    };
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"{policy_name}\"\ndefault_branch = \"{default_branch}\"\n\n[[stacks.rust.configurations]]\nname = \"{configuration_name}\"\nfeatures = [\"{feature}\"]\nall_features = false\ntarget = \"host\"\n"
    );
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        workflow_configuration_fixture::git(&["branch", "-m", "main"], root)?;
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
        write_sdk_workspace(root, &["demo", "maintenance"])?;
    }
    let mut root_manifest = fs::read_to_string(root.join("Cargo.toml"))?;
    let members = if policy == WorkflowPolicy::VelnorRepositoryV1 {
        "\"crates/maintenance\", \"crates/velnor-actions-cli\", \"crates/velnor-archive-guard\""
    } else {
        "\"crates/maintenance\""
    };
    write!(
        root_manifest,
        "\n[workspace]\nmembers = [{members}]\nresolver = \"2\"\n"
    )?;
    fs::write(root.join("Cargo.toml"), root_manifest)?;
    write_owner_member(root, tagged)?;
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        workflow_configuration_fixture::git(
            &[
                "add",
                "--",
                "Cargo.toml",
                "Cargo.lock",
                "crates/velnor-actions-cli",
                "crates/velnor-archive-guard",
            ],
            root,
        )?;
    }
    workflow_configuration_fixture::write_nextest_task(root)?;
    workflow_configuration_fixture::write_nextest_task(&root.join("crates/maintenance"))?;
    Ok(repo)
}

pub(super) fn write_sdk_workspace(root: &Path, packages: &[&str]) -> Result<(), Box<dyn Error>> {
    let version = env!("CARGO_PKG_VERSION");
    let mut lock = "version = 4\n\n".to_owned();
    for name in packages {
        let package_version = match *name {
            "velnor-actions-cli" | "velnor-archive-guard" => version,
            _ => "0.1.0",
        };
        write!(
            lock,
            "[[package]]\nname = \"{name}\"\nversion = \"{package_version}\"\n\n"
        )?;
    }
    for name in ["velnor-actions-cli", "velnor-archive-guard"] {
        write!(
            lock,
            "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n\n"
        )?;
    }
    fs::write(root.join("Cargo.lock"), lock)?;
    write_sdk_member(root, "velnor-actions-cli", "velnor-actions")?;
    write_sdk_member(root, "velnor-archive-guard", "velnor-archive-guard")?;
    Ok(())
}

pub(super) fn rename_root_as_cli(root: &Path) -> Result<(), Box<dyn Error>> {
    let manifest = root.join("Cargo.toml");
    let contents = fs::read_to_string(&manifest)?;
    let package = format!(
        "[package]\nname = \"velnor-actions-cli\"\nversion = \"{}\"\nedition = \"2024\"\n\n[[bin]]\nname = \"velnor-actions\"\npath = \"src/main.rs\"\n",
        env!("CARGO_PKG_VERSION")
    );
    let workspace = contents.split_once("\n[workspace]").map(|(_, rest)| rest);
    let mut members = workspace.map_or_else(String::new, str::to_owned);
    members = members.replace(", \"crates/velnor-actions-cli\"", "");
    fs::write(manifest, format!("{package}\n[workspace]{members}\n"))?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/main.rs"), "fn main() {}\n")?;
    let cli = root.join("crates/velnor-actions-cli");
    if cli.exists() {
        fs::remove_dir_all(cli)?;
    }
    let version = env!("CARGO_PKG_VERSION");
    fs::write(
        root.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"velnor-actions-cli\"\nversion = \"{version}\"\n\n[[package]]\nname = \"maintenance\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"velnor-archive-guard\"\nversion = \"{version}\"\n"
        ),
    )?;
    workflow_configuration_fixture::git(&["add", "-A"], root)?;
    Ok(())
}

fn write_sdk_member(root: &Path, package: &str, binary: &str) -> Result<(), Box<dyn Error>> {
    let directory = root.join("crates").join(package);
    fs::create_dir_all(directory.join("src"))?;
    fs::write(
        directory.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{package}\"\nversion = \"{}\"\nedition = \"2024\"\n\n[[bin]]\nname = \"{binary}\"\npath = \"src/main.rs\"\n",
            env!("CARGO_PKG_VERSION")
        ),
    )?;
    fs::write(directory.join("src/main.rs"), "fn main() {}\n")?;
    Ok(())
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
