// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GeneratedWorkflowKind, inputs::generated::GeneratedOwner};
use crate::verification_jobs::{StackJobKind, WorkflowTaskPolicy};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{ReleaseTarget, VelnorConfig, WorkflowTask};

type Sources = BTreeMap<String, Option<String>>;

pub(super) fn matches(
    config: &VelnorConfig,
    generated: &BTreeMap<String, GeneratedOwner>,
    sources: &Sources,
    policy: &WorkflowTaskPolicy,
) -> bool {
    match policy {
        WorkflowTaskPolicy::Verification(policy) => {
            let scale = config.execution.as_ref().map(|execution| execution.scale_selector().map(|selector| selector.token())).transpose();
            config.workflow.tasks.contains(&WorkflowTask::Verification(policy.task.clone()))
                && policy.runner_label == policy.task.runner.runs_on()
                && scale.is_ok_and(|scale| scale == policy.scale_set_token)
                && approved_mise(config, policy.task.runner.mise_target(), &policy.mise_setup)
                && source(sources, &policy.task.source.mise_config) == Some(&policy.mise_config_sha256)
                && source(sources, &policy.task.source.mise_lock_path()) == Some(&policy.mise_lock_sha256)
                && source(sources, &policy.task.source.rust_toolchain_path()) == Some(&policy.rust_toolchain_sha256)
        }
        WorkflowTaskPolicy::Build(policy) => {
            config.workflow.tasks.contains(&WorkflowTask::Build(policy.task.clone()))
                && policy.runner_label == policy.task.runner.runs_on()
                && approved_mise(config, policy.task.runner.mise_target(), &policy.mise_setup)
                && source(sources, "mise.toml").and_then(Option::as_ref) == Some(&policy.mise_config_sha256)
                && source(sources, "mise.lock").and_then(Option::as_ref) == Some(&policy.mise_lock_sha256)
                && source(sources, "rust-toolchain.toml").and_then(Option::as_ref) == Some(&policy.rust_toolchain_sha256)
                && source(sources, &policy.task.source.mise_config).and_then(Option::as_ref) == Some(&policy.source_mise_config_sha256)
                && local_source(sources, &policy.task.source.mise_lock_path(), "mise.lock", policy.source_mise_lock_sha256.as_ref())
                && local_source(sources, &policy.task.source.rust_toolchain_path(), "rust-toolchain.toml", policy.source_rust_toolchain_sha256.as_ref())
        }
        WorkflowTaskPolicy::NativeImage(policy) => {
            config.workflow.tasks.contains(&WorkflowTask::NativeImage(policy.task.clone()))
                && policy.runner_label == policy.task.platform.runs_on()
                && source(sources, &policy.task.script).and_then(Option::as_ref) == Some(&policy.source_sha256)
        }
        WorkflowTaskPolicy::Image(policy) => config.images.iter().any(|image| matches!((velnor_actions_contract::canonical_json_bytes(image), velnor_actions_contract::canonical_json_bytes(&policy.image)), (Ok(expected), Ok(actual)) if expected == actual)),
        WorkflowTaskPolicy::Bun(policy) => generated.get(&policy.id).is_some_and(|owner| owner.kind == GeneratedWorkflowKind::BunMember && owner.member.as_deref() == Some(policy.name.as_str()) && policy.assets == owner.assets),
        WorkflowTaskPolicy::BunSuite(policy) => generated.get(&policy.id).is_some_and(|owner| owner.kind == GeneratedWorkflowKind::BunSuite && owner.member.as_deref() == Some(policy.member.as_str()) && owner.suite.as_deref() == Some(policy.suite.as_str()) && policy.required == owner.required.contains(&policy.id) && policy.report.is_some() == owner.report && policy.assets == owner.assets && policy.job_ids().into_iter().collect::<BTreeSet<_>>() == owner.jobs),
        WorkflowTaskPolicy::BunAssets(policy) => generated.get(&policy.id).is_some_and(|owner| owner.kind == GeneratedWorkflowKind::BunAssets && owner.member.as_deref() == Some(policy.member.as_str())),
        WorkflowTaskPolicy::Stack(policy) => generated.get(&policy.id).is_some_and(|owner| matches!((policy.kind, owner.kind), (StackJobKind::Dbt, GeneratedWorkflowKind::Dbt) | (StackJobKind::Ansible, GeneratedWorkflowKind::Ansible))) && approved_mise(config, ReleaseTarget::LinuxX86_64.triple(), &policy.setup),
    }
}

fn source<'a>(sources: &'a Sources, path: &str) -> Option<&'a Option<String>> {
    sources.get(path)
}

fn approved_mise(config: &VelnorConfig, target: &str, mise: &crate::MiseSetup) -> bool {
    let target = ReleaseTarget::ALL
        .into_iter()
        .find(|known| known.triple() == target);
    target.is_some_and(|target| {
        super::tools::mise_setup(config, target).is_ok_and(|expected| *mise == expected)
    })
}

fn local_source(sources: &Sources, path: &str, root: &str, actual: Option<&String>) -> bool {
    if path == root {
        actual.is_none()
    } else {
        source(sources, path).map(Option::as_ref) == Some(actual)
    }
}
