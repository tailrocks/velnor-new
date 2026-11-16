// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_orchestrator::{GenerationPreparation, prepare};

use super::feature_toggles::feature_toggle_repo;

const BASE: &str = "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[[stacks.rust.configurations]]\nname = \"default\"\nfeatures = [\"default\"]\ntarget = \"host\"\n\n[[stacks.rust.configurations]]\nname = \"scoped\"\nfeatures = [\"maintenance-only\"]\ntarget = \"host\"\n";

fn write_config(root: &Path, scope: &str) -> Result<(), Box<dyn Error>> {
    fs::write(root.join(".velnor/config.toml"), format!("{BASE}{scope}"))?;
    Ok(())
}

fn configuration_manifests(
    prepared: &GenerationPreparation,
    configuration: &str,
) -> BTreeSet<String> {
    prepared
        .discovery
        .proposals
        .iter()
        .filter(|task| {
            task.stack_id == "rust"
                && task.configuration == configuration
                && !task.identity.unit_id.is_empty()
        })
        .map(|task| task.identity.unit_path.clone())
        .collect()
}

#[test]
fn unscoped_configuration_applies_to_every_package() -> Result<(), Box<dyn Error>> {
    let repo = feature_toggle_repo(false, false)?;
    write_config(repo.path(), "")?;
    let prepared = prepare(repo.path())?;
    assert_eq!(configuration_manifests(&prepared, "scoped").len(), 2);
    Ok(())
}

#[test]
fn scoped_configuration_runs_only_for_its_packages() -> Result<(), Box<dyn Error>> {
    let repo = feature_toggle_repo(false, false)?;
    write_config(repo.path(), "packages = [\"maintenance\"]\n")?;
    let prepared = prepare(repo.path())?;
    assert_eq!(
        configuration_manifests(&prepared, "scoped"),
        BTreeSet::from(["crates/maintenance/Cargo.toml".to_owned()])
    );
    assert_eq!(configuration_manifests(&prepared, "default").len(), 2);
    Ok(())
}

#[test]
fn scoped_configuration_naming_an_unknown_package_is_refused() -> Result<(), Box<dyn Error>> {
    let repo = feature_toggle_repo(false, false)?;
    write_config(repo.path(), "packages = [\"nonexistent\"]\n")?;
    let error = prepare(repo.path())
        .err()
        .ok_or("unknown package accepted")?;
    assert!(
        error
            .to_string()
            .contains("unknown_package:scoped:nonexistent"),
        "{error}"
    );
    Ok(())
}

#[test]
fn scoped_configuration_with_a_blank_or_repeated_package_is_refused() -> Result<(), Box<dyn Error>>
{
    let repo = feature_toggle_repo(false, false)?;
    for scope in ["packages = [\" \"]\n", "packages = [\"demo\", \"demo\"]\n"] {
        write_config(repo.path(), scope)?;
        assert!(prepare(repo.path()).is_err(), "{scope}");
    }
    Ok(())
}

#[test]
fn scoped_configuration_adds_nothing_for_a_workspace_without_its_packages()
-> Result<(), Box<dyn Error>> {
    let repo = feature_toggle_repo(false, false)?;
    let tool = repo.path().join("tools/standalone");
    fs::create_dir_all(tool.join("src"))?;
    fs::write(
        tool.join("Cargo.toml"),
        "[package]\nname = \"standalone\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )?;
    fs::write(tool.join("src/lib.rs"), "")?;
    write_config(repo.path(), "packages = [\"maintenance\"]\n")?;
    let prepared = prepare(repo.path())?;
    let scoped: BTreeSet<&str> = prepared
        .discovery
        .proposals
        .iter()
        .filter(|task| task.stack_id == "rust" && task.configuration == "scoped")
        .map(|task| task.identity.unit_path.as_str())
        .collect();
    assert_eq!(scoped, BTreeSet::from(["crates/maintenance/Cargo.toml"]));
    Ok(())
}

#[test]
fn scoped_configuration_naming_only_an_ineligible_package_adds_no_task()
-> Result<(), Box<dyn Error>> {
    let repo = feature_toggle_repo(false, false)?;
    write_config(repo.path(), "packages = [\"maintenance\"]\n")?;
    let mut config = fs::read_to_string(repo.path().join(".velnor/config.toml"))?.replace(
        "features = [\"maintenance-only\"]",
        "features = [\"default\"]",
    );
    config.push_str("\n[discovery]\nexclude = [\"crates/maintenance/**\"]\n");
    fs::write(repo.path().join(".velnor/config.toml"), config)?;
    let prepared = prepare(repo.path())?;
    let scoped: Vec<&str> = prepared
        .discovery
        .proposals
        .iter()
        .filter(|task| task.stack_id == "rust" && task.configuration == "scoped")
        .map(|task| task.task_id.as_str())
        .collect();
    assert!(scoped.is_empty(), "{scoped:?}");
    Ok(())
}

#[test]
fn scoped_configuration_requests_features_only_its_packages_declare() -> Result<(), Box<dyn Error>>
{
    let repo = feature_toggle_repo(false, false)?;
    write_config(repo.path(), "packages = [\"demo\"]\n")?;
    let error = prepare(repo.path())
        .err()
        .ok_or("feature declared only out of scope accepted")?;
    assert!(
        error
            .to_string()
            .contains("unknown_feature:scoped:demo:maintenance-only"),
        "{error}"
    );
    Ok(())
}

#[test]
fn scoped_configuration_is_accepted_when_rust_is_ignored() -> Result<(), Box<dyn Error>> {
    let repo = feature_toggle_repo(false, false)?;
    write_config(repo.path(), "packages = [\"maintenance\"]\n")?;
    let mut config = fs::read_to_string(repo.path().join(".velnor/config.toml"))?;
    config.push_str("\n[stacks]\nignore = [\"rust\"]\n");
    fs::write(repo.path().join(".velnor/config.toml"), config)?;
    let prepared = prepare(repo.path())?;
    assert!(
        prepared
            .discovery
            .proposals
            .iter()
            .all(|task| task.stack_id != "rust")
    );
    Ok(())
}

#[test]
fn scoped_configuration_naming_only_an_ineligible_package_still_validates_features()
-> Result<(), Box<dyn Error>> {
    let repo = feature_toggle_repo(false, false)?;
    write_config(repo.path(), "packages = [\"maintenance\"]\n")?;
    let mut config = fs::read_to_string(repo.path().join(".velnor/config.toml"))?;
    config = config.replace("features = [\"maintenance-only\"]", "features = [\"typo\"]");
    config.push_str("\n[discovery]\nexclude = [\"crates/maintenance/**\"]\n");
    fs::write(repo.path().join(".velnor/config.toml"), config)?;
    let error = prepare(repo.path())
        .err()
        .ok_or("undeclared feature accepted")?;
    assert!(
        error.to_string().contains("unknown_feature:scoped"),
        "{error}"
    );
    Ok(())
}
