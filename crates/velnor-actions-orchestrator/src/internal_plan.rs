//! Plan-shape helpers for `plan-v1`: packages, metadata, and identity.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{ExecuteTaskIds, ExecuteTaskRef, PlanGenerator, PlanPackage};
use velnor_actions_rust::TaskGroup;

use crate::discover::Discovery;

/// Manifest path for a manifest key.
pub(crate) fn manifest_for_key(key: &str) -> String {
    if key == "root" {
        "Cargo.toml".to_owned()
    } else {
        format!("{key}/Cargo.toml")
    }
}

/// Opaque adapter metadata for one matrix entry.
pub(crate) fn adapter_metadata(group: &TaskGroup) -> serde_json::Value {
    serde_json::json!({
        "package_id": group.package_id,
        "package_name": group.package_name,
        "manifest_key": group.manifest_key,
        "kind": group.kind.as_str(),
        "configuration": group.configuration,
        "target": group.target,
    })
}

/// Single executable obligation named by kind.
pub(crate) fn execute_ids(group: &TaskGroup) -> ExecuteTaskIds {
    ExecuteTaskIds {
        tasks: BTreeMap::from([(
            group.kind.as_str().to_owned(),
            ExecuteTaskRef::Single(group.task_id.clone()),
        )]),
    }
}

/// Default generator identity when the request omits it.
pub(crate) fn default_generator() -> PlanGenerator {
    PlanGenerator {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        sha256: "0".repeat(64),
    }
}

/// Complete package inventory with selection flags.
pub(crate) fn plan_packages(discovery: &Discovery, selected: &BTreeSet<&str>) -> Vec<PlanPackage> {
    let mut packages = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if !package.in_workspace || package.external {
                continue;
            }
            let is_selected = selected.contains(package.id.as_str());
            let mut tasks: Vec<String> = discovery
                .task_groups
                .iter()
                .filter(|group| group.package_id == package.id)
                .map(|group| group.task_id.clone())
                .collect();
            tasks.sort();
            packages.push(PlanPackage {
                package_id: package.id.clone(),
                name: package.name.clone(),
                manifest: package.manifest.clone(),
                selected: is_selected,
                reasons: vec![if is_selected {
                    "selected".to_owned()
                } else {
                    "not_affected".to_owned()
                }],
                tasks,
            });
        }
    }
    packages.sort_by(|left, right| left.package_id.cmp(&right.package_id));
    packages
}
