// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::scoped_workspace::{PACKAGES, TestResult, commit, plan, put, set};
use super::{maintenance_fixture, maintenance_inputs, maintenance_non_inputs, repository_root};

const FRESHNESS_INVENTORY: &str = ".velnor/freshness-inventory.json";
const MAINTENANCE_CHECK: &str = "check-maintenance-helpers";

#[test]
fn freshness_inventory_alone_selects_only_the_maintenance_check() -> TestResult {
    let root = repository_root()?;
    let inputs = maintenance_inputs(&root)?;
    assert!(inputs.contains(FRESHNESS_INVENTORY));
    let repo = maintenance_fixture(&inputs, &maintenance_non_inputs(&root)?)?;
    put(repo.root(), FRESHNESS_INVENTORY, "{\"changed\":true}\n")?;
    let head = commit(repo.root(), "freshness inventory only")?;
    let run = plan(repo.root(), "pull_request", Some(&repo.base), &head)?;
    assert_eq!(run.with_disposition("not_affected")?, set(&PACKAGES));
    assert!(run.with_disposition("run")?.is_empty());
    assert_eq!(
        run.jobs()?.get(MAINTENANCE_CHECK).map(String::as_str),
        Some("run")
    );
    assert!(run.warnings().is_empty(), "{:?}", run.warnings());
    Ok(())
}

#[test]
fn other_velnor_paths_still_select_every_package() -> TestResult {
    let root = repository_root()?;
    let inputs = maintenance_inputs(&root)?;
    let repo = maintenance_fixture(&inputs, &maintenance_non_inputs(&root)?)?;
    let config = std::fs::read_to_string(repo.root().join(".velnor/config.toml"))?;
    put(
        repo.root(),
        ".velnor/config.toml",
        &format!("{config}# changed\n"),
    )?;
    let head = commit(repo.root(), "velnor config")?;
    let run = plan(repo.root(), "pull_request", Some(&repo.base), &head)?;
    assert_eq!(run.with_disposition("run")?, set(&PACKAGES));
    assert!(
        run.warnings()
            .iter()
            .any(|w| w == "global_config_changed:selecting_all"),
        "{:?}",
        run.warnings()
    );
    Ok(())
}
