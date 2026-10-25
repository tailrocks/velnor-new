use std::collections::BTreeSet;

use velnor_actions_contract::PlanPackage;

use crate::discover::Discovery;

/// Complete package inventory with selection flags.
///
/// Rust rows enumerate workspace packages; tofu rows enumerate
/// selected tofu projects by root key, so an untouched root stays
/// distinguishable (`not_affected`) instead of vanishing.
pub(crate) fn plan_packages(discovery: &Discovery, selected: &BTreeSet<&str>) -> Vec<PlanPackage> {
    let mut packages = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if !package.in_workspace || package.external {
                continue;
            }
            packages.push(package_row(
                &package.id,
                &package.name,
                &package.manifest,
                discovery,
                selected,
            ));
        }
    }
    packages.extend(tofu_packages(discovery, selected));
    packages.extend(discovery.mise_checks.iter().map(|item| {
        package_row(
            &item.proposal.identity.unit_id,
            &item.check.id,
            &item.proposal.identity.unit_path,
            discovery,
            selected,
        )
    }));
    packages.extend(discovery.native_checks.iter().map(|check| {
        package_row(
            &check.id,
            &check.id,
            "homebrew-native-check",
            discovery,
            selected,
        )
    }));
    packages.sort_by(|left, right| left.package_id.cmp(&right.package_id));
    packages
}

/// One inventory row: selection flag plus sorted task IDs.
fn package_row(
    package_id: &str,
    name: &str,
    manifest: &str,
    discovery: &Discovery,
    selected: &BTreeSet<&str>,
) -> PlanPackage {
    let is_selected = selected.contains(package_id);
    let mut tasks: Vec<String> = discovery
        .proposals
        .iter()
        .filter(|task| task.identity.unit_id == package_id)
        .map(|task| task.task_id.clone())
        .collect();
    tasks.sort();
    PlanPackage {
        package_id: package_id.to_owned(),
        name: name.to_owned(),
        manifest: manifest.to_owned(),
        selected: is_selected,
        reasons: vec![if is_selected {
            "selected".to_owned()
        } else {
            "not_affected".to_owned()
        }],
        tasks,
    }
}

/// Tofu inventory rows: one per selected project, keyed by root key.
fn tofu_packages(discovery: &Discovery, selected: &BTreeSet<&str>) -> Vec<PlanPackage> {
    let mut roots = crate::select_tofu::tofu_selected_roots(&discovery.statuses);
    roots.dedup();
    roots
        .iter()
        .map(|root| {
            let key = velnor_actions_tofu::key_for_root(root);
            let display = velnor_actions_tofu::display_for_root(root);
            package_row(&key, &display, &display, discovery, selected)
        })
        .collect()
}
