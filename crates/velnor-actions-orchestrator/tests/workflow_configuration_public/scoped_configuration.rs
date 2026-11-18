// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_orchestrator::prepare;

use super::feature_toggles::feature_toggle_repo;

const BASE: &str = "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[[stacks.rust.configurations]]\nname = \"default\"\nfeatures = [\"default\"]\ntarget = \"host\"\n\n[[stacks.rust.configurations]]\nname = \"scoped\"\nfeatures = [\"maintenance-only\"]\ntarget = \"host\"\n";

fn write_config(root: &Path, scope: &str) -> Result<(), Box<dyn Error>> {
    fs::write(root.join(".velnor/config.toml"), format!("{BASE}{scope}"))?;
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
