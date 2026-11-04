// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::workflow::cache_family::{RootSlug, S256, StateUnit};
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_ROOT_ENV, MBX_BUNDLE_SNAPSHOT_ENV,
};
use velnor_actions_contract::{Job, StepKind, StepRole};
use velnor_actions_workflow_renderer::plan_sources::cache_keys::StateRootPlan;
use velnor_actions_workflow_renderer::plan_sources::state_select::{SealRoot, StateSealPlan};

use crate::OrchestratorError;
use crate::affected_inputs::InputRule;
use crate::discover::{Discovery, PlannedWorkspace};
use crate::select_affected::manifest_dir;

const ROOTLESS_SOURCE_DIRS: [&str; 4] = ["src", "tests", "benches", "examples"];

pub(crate) fn plans(
    discovery: &Discovery,
    jobs: &BTreeMap<String, Job>,
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
            })
        })
        .collect()
}

pub(crate) fn seal_plan(
    jobs: &BTreeMap<String, Job>,
    policy: WorkflowPolicy,
) -> Result<StateSealPlan, OrchestratorError> {
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
    Ok(StateSealPlan {
        roots,
        canary: policy == WorkflowPolicy::VelnorRepositoryV1,
    })
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
            specs.insert("build.rs".to_owned());
            specs.extend(
                ROOTLESS_SOURCE_DIRS
                    .iter()
                    .map(|name| format!(":(glob){name}/**")),
            );
        } else {
            specs.insert(dir);
        }
    }
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
