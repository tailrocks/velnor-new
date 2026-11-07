// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::cache_family::{RootSlug, S256, StateUnit};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_ROOT_ENV, MBX_BUNDLE_SNAPSHOT_ENV,
};
use velnor_actions_contract::workflow::state_seal::state_producer_job_id;
use velnor_actions_contract::{Job, StepKind, StepRole};
use velnor_actions_workflow_renderer::plan_sources::cache_keys::StateRootPlan;
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, StateSealPlan};

use crate::OrchestratorError;
use crate::planner::affected_inputs::InputRule;
use crate::planner::discover::{Discovery, PlannedWorkspace};
use crate::planner::select_affected::manifest_dir;

pub(crate) fn plans(
    discovery: &Discovery,
    jobs: &BTreeMap<String, Job>,
    recipes: &BTreeMap<String, S256>,
) -> Result<Vec<StateRootPlan>, OrchestratorError> {
    let mut digests: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for job in jobs.values() {
        let Some(root) = union_root(job) else {
            continue;
        };
        let lines = digests.entry(root).or_default();
        for step in &job.steps {
            if let StepKind::TaskExecution {
                task_id,
                task_digest,
                ..
            } = &step.kind
            {
                lines.insert(format!("{task_id}\t{task_digest}"));
            }
        }
    }
    let roots: Vec<&str> = digests.keys().map(String::as_str).collect();
    RootSlug::unique(&roots)?;
    digests
        .iter()
        .map(|(root, lines)| {
            let repo_root = if root == "." { "" } else { root.as_str() };
            let text = lines.iter().fold(String::new(), |mut text, line| {
                text.push_str(line);
                text.push('\n');
                text
            });
            Ok(StateRootPlan {
                root: repo_root.to_owned(),
                pathspecs: rust_inputs(discovery, repo_root),
                task_set: S256::of(text.as_bytes()),
                producer_recipe: recipes
                    .get(&state_producer_job_id(&RootSlug::from_root(repo_root)?))
                    .copied()
                    .ok_or_else(|| OrchestratorError::Contract {
                        problem: format!("state_producer_recipe_missing:{root}"),
                    })?,
            })
        })
        .collect()
}

pub(crate) fn seal_plan(jobs: &BTreeMap<String, Job>) -> Result<StateSealPlan, OrchestratorError> {
    let mut roots: Vec<SealRoot> = Vec::new();
    for job in jobs.values() {
        let Some(root) = union_root(job) else {
            continue;
        };
        let seal = SealRoot {
            unit: StateUnit::Union,
            slug: RootSlug::from_root(&root)?,
        };
        if !roots.contains(&seal) {
            roots.push(seal);
        }
    }
    Ok(StateSealPlan { roots })
}

fn union_root(job: &Job) -> Option<String> {
    job.steps.iter().find_map(|step| {
        if step.role != Some(StepRole::MbxBundleKey) {
            return None;
        }
        let StepKind::Shell { env, .. } = &step.kind else {
            return None;
        };
        env.contains_key(MBX_BUNDLE_SNAPSHOT_ENV)
            .then(|| env.get(MBX_BUNDLE_ROOT_ENV).cloned())
            .flatten()
    })
}

fn rust_inputs(discovery: &Discovery, root: &str) -> Vec<String> {
    let workspace = discovery
        .workspaces
        .iter()
        .find(|workspace| same_root(&workspace.record.workspace_root, root));
    let mut specs = BTreeSet::new();
    let join = |dir: &str, name: &str| {
        if dir.is_empty() {
            name.to_owned()
        } else {
            format!("{dir}/{name}")
        }
    };
    let mut dir = root.to_owned();
    loop {
        specs.insert(join(&dir, ".cargo"));
        specs.insert(format!(":(glob){}", join(&dir, "rust-toolchain*")));
        match dir.rsplit_once('/') {
            Some((parent, _)) => dir = parent.to_owned(),
            None if dir.is_empty() => break,
            None => dir = String::new(),
        }
    }
    specs.insert(join(root, "Cargo.toml"));
    if let Some(workspace) = workspace {
        member_specs(workspace, &mut specs);
        rule_specs(discovery, workspace, &mut specs);
    }
    specs.into_iter().collect()
}

fn member_specs(workspace: &PlannedWorkspace, specs: &mut BTreeSet<String>) {
    for package in &workspace.record.packages {
        if !package.in_workspace || package.external {
            continue;
        }
        let dir = manifest_dir(&package.manifest);
        if dir.is_empty() {
            specs.insert("Cargo.toml".to_owned());
        } else {
            specs.insert(dir.clone());
        }
        for target in &package.targets {
            target_spec(&dir, &target.src_path, specs);
        }
    }
}

fn normalized(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn target_spec(package_dir: &str, src_path: &str, specs: &mut BTreeSet<String>) {
    if src_path.starts_with('/') {
        return;
    }
    let Some(src_path) = normalized(src_path) else {
        return;
    };
    let src_path = src_path.as_str();
    if !package_dir.is_empty() && src_path.starts_with(&format!("{package_dir}/")) {
        return;
    }
    match src_path.rsplit_once('/') {
        Some((parent, _)) => specs.insert(format!(":(glob){parent}/**")),
        None => specs.insert(src_path.to_owned()),
    };
}

fn rule_specs(discovery: &Discovery, workspace: &PlannedWorkspace, specs: &mut BTreeSet<String>) {
    let ids: BTreeSet<&str> = workspace
        .record
        .packages
        .iter()
        .map(|package| package.id.as_str())
        .collect();
    for rule in &discovery.affected.rules {
        if rule_touches(rule, &ids) {
            specs.extend(rule.globs.iter().map(|glob| format!(":(glob){glob}")));
        }
    }
}

fn rule_touches(rule: &InputRule, ids: &BTreeSet<&str>) -> bool {
    rule.package_ids.iter().any(|id| ids.contains(id.as_str()))
}

fn same_root(workspace_root: &str, root: &str) -> bool {
    let normal = |text: &str| text.trim_matches('/').trim_start_matches("./").to_owned();
    let (left, right) = (normal(workspace_root), normal(root));
    left == right || (left == "." && right.is_empty()) || (left.is_empty() && right == ".")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::target_spec;

    fn specs(package_dir: &str, paths: &[&str]) -> Vec<String> {
        let mut specs = BTreeSet::new();
        for path in paths {
            target_spec(package_dir, path, &mut specs);
        }
        specs.into_iter().collect()
    }

    #[test]
    fn root_package_targets_name_their_own_directories_and_the_build_script() {
        let got = specs(
            "",
            &[
                "src/lib.rs",
                "src/main.rs",
                "tools/build.rs",
                "lib2/extra.rs",
                "tests/a.rs",
                "build.rs",
                "/outside/the/repo.rs",
                "../outside/the/repo.rs",
            ],
        );
        assert_eq!(
            got,
            [
                ":(glob)lib2/**",
                ":(glob)src/**",
                ":(glob)tests/**",
                ":(glob)tools/**",
                "build.rs",
            ]
        );
    }

    #[test]
    fn nested_package_targets_inside_the_package_directory_add_nothing() {
        assert_eq!(
            specs(
                "crates/a",
                &[
                    "crates/a/src/lib.rs",
                    "crates/a/../shared/lib.rs",
                    "crates/a/./src/x.rs",
                ]
            ),
            [":(glob)crates/shared/**"]
        );
    }
}
