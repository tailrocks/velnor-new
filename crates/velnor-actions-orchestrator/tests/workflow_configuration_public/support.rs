// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as _;

use velnor_actions_contract::{Job, StepKind};
use velnor_actions_orchestrator::GenerationPreparation;

use super::workflow_configuration_fixture;

pub(super) fn discovered_rust_packages(
    prepared: &GenerationPreparation,
) -> (BTreeMap<&str, &str>, BTreeSet<&str>) {
    let runnable_tasks: Vec<_> = prepared
        .discovery
        .proposals
        .iter()
        .filter(|task| {
            task.stack_id == "rust" && !task.no_targets && !task.identity.unit_id.is_empty()
        })
        .collect();
    let unit_paths: BTreeMap<_, _> = runnable_tasks
        .iter()
        .map(|task| (task.task_id.as_str(), task.identity.unit_path.as_str()))
        .collect();
    assert_eq!(unit_paths.len(), runnable_tasks.len());
    let expected_packages = BTreeSet::from(["Cargo.toml", "crates/nested/Cargo.toml"]);
    assert_eq!(
        unit_paths.values().copied().collect::<BTreeSet<_>>(),
        expected_packages
    );
    (unit_paths, expected_packages)
}

pub(super) fn config_with_timeouts(
    plan_timeout: u16,
    crate_timeout: Option<u16>,
) -> Result<String, std::fmt::Error> {
    let mut config = workflow_configuration_fixture::config_with_verification_branch(None);
    writeln!(config, "plan_timeout_minutes = {plan_timeout}")?;
    if let Some(crate_timeout) = crate_timeout {
        writeln!(config, "crate_timeout_minutes = {crate_timeout}")?;
    }
    Ok(config)
}

pub(super) fn rust_owner_jobs(prepared: &GenerationPreparation) -> Vec<(&str, &Job)> {
    prepared
        .workflow
        .ir
        .jobs
        .iter()
        .filter(|(id, job)| id.starts_with("rust-") && !job.is_dependency_seed())
        .map(|(id, job)| (id.as_str(), job))
        .collect()
}

/// Map a branch job id to its logical owner: reader/writer branches run on
/// mutually exclusive events, so a branch pair counts as one owner. The
/// counterpart must exist, so naturally suffixed group ids never mis-group.
pub(super) fn logical_owner_id<'a>(all: &BTreeSet<&'a str>, id: &'a str) -> &'a str {
    use velnor_actions_contract::workflow::crate_job::{
        GROUP_READER_SUFFIX, GROUP_WRITER_SUFFIX, reader_job_id_for, writer_job_id_for,
    };
    for suffix in [GROUP_READER_SUFFIX, GROUP_WRITER_SUFFIX] {
        if let Some(base) = id.strip_suffix(suffix) {
            let counterpart = if suffix == GROUP_READER_SUFFIX {
                writer_job_id_for(base)
            } else {
                reader_job_id_for(base)
            };
            if all.contains(counterpart.as_str()) {
                return base;
            }
        }
    }
    id
}

pub(super) fn task_execution_ids(job: &Job) -> impl Iterator<Item = &str> {
    job.steps.iter().filter_map(|step| match &step.kind {
        StepKind::TaskExecution { task_id, .. } => Some(task_id.as_str()),
        _ => None,
    })
}

pub(super) fn task_execution_steps(job: &Job) -> Vec<(usize, &str, &str, bool, Option<&str>)> {
    job.steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| match &step.kind {
            StepKind::TaskExecution { task_id, env, .. } => Some((
                index,
                task_id.as_str(),
                step.name.as_str(),
                step.parallel,
                env.get("CARGO_TARGET_DIR").map(String::as_str),
            )),
            _ => None,
        })
        .collect()
}

pub(super) fn rendered_job_block<'a>(
    workflow: &'a str,
    job_id: &str,
) -> Result<&'a str, Box<dyn Error>> {
    let marker = format!("  {job_id}:\n");
    let start = workflow
        .find(&marker)
        .ok_or_else(|| format!("rendered job {job_id} is missing"))?;
    let tail = &workflow[start..];
    let mut end = tail.len();
    let mut offset = 0;
    for (index, line) in tail.split_inclusive('\n').enumerate() {
        if index > 0
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':')
        {
            end = offset;
            break;
        }
        offset += line.len();
    }
    Ok(&tail[..end])
}
