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
