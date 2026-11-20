// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{MisePlatform, looks_github_hosted};

use crate::check::runner_label_of;
use crate::error::{WorkflowsError, refuse};
use crate::plan::{Action, TrustedPool, Workflow, WorkflowsPlan};
use crate::raw::{RawStep, RawWorkflow};

const REF_INPUTS: &[&str] = &["ref", "sha", "repository"];
const CHECKOUT_KEYS: &[&str] = &["ref", "repository"];
const SERVER_KEY: &str = "github-server-url";
const DOCKER_SOURCE_KEYS: &[&str] = &["context", "file", "build-contexts", "build-args"];
const DOCKER_PATH_KEYS: &[&str] = &["context", "file"];
const DOCKER_CONTEXTS_KEY: &str = "build-contexts";
const WORKSPACE: &str = "${{ github.workspace }}";

fn reads_input(value: &str) -> bool {
    value.contains("inputs.") || value.contains("github.event.inputs")
}

fn reads_step_or_job(value: &str) -> bool {
    value.contains("steps.") || value.contains("needs.")
}

fn reads_matrix(value: &str) -> bool {
    value.contains("matrix.")
}

fn names(label: &str, value: &str) -> bool {
    value.eq_ignore_ascii_case(label)
}

fn names_label(raw: &RawWorkflow, label: &str) -> bool {
    raw.job.iter().any(|job| {
        job.runner
            .as_deref()
            .is_some_and(|runner| names(label, runner))
            || job.runner_labels.keys().any(|key| names(label, key))
            || job.matrix.as_ref().is_some_and(|matrix| {
                matrix
                    .axes
                    .iter()
                    .flat_map(|axes| axes.values().flatten())
                    .chain(matrix.include.iter().flat_map(|entry| entry.values()))
                    .any(|value| names(label, value))
            })
    })
}

fn runs_on(workflow: &Workflow, label: &str) -> bool {
    names(label, &workflow.runner)
        || workflow.jobs.iter().any(|job| {
            job.runner
                .as_deref()
                .is_some_and(|runner| names(label, runner))
                || job.matrix_runner.as_ref().is_some_and(|matrix| {
                    matrix.labels.iter().any(|(runner, _)| names(label, runner))
                })
        })
}

pub(crate) fn reserved_across(workflows: &[Workflow]) -> Result<(), WorkflowsError> {
    for owner in workflows {
        let Some(pool) = &owner.trusted else {
            continue;
        };
        if let Some(user) = workflows
            .iter()
            .find(|other| other.file != owner.file && runs_on(other, &pool.label))
        {
            return refuse(format!(
                "the workflow {} runs a job on {}, the trusted_runner of {}, which would run pull request code on the trusted pool outside the trusted event check",
                user.file, pool.label, owner.file
            ));
        }
    }
    Ok(())
}

impl WorkflowsPlan {
    pub fn reserve_against(
        &self,
        runners: &[&str],
        trusted: &[&str],
    ) -> Result<(), WorkflowsError> {
        for workflow in &self.workflows {
            if let Some(pool) = &workflow.trusted
                && runners.iter().any(|runner| names(&pool.label, runner))
            {
                return refuse(format!(
                    "the trusted_runner {} of the workflow {} is the runner of a job generated from rust-ci.toml, which runs pull request code",
                    pool.label, workflow.file
                ));
            }
            if let Some(label) = trusted.iter().find(|label| runs_on(workflow, label)) {
                return refuse(format!(
                    "the workflow {} runs a job on {label}, the trusted_runner of rust-ci.toml, which would run pull request code on the trusted pool outside the trusted event check",
                    workflow.file
                ));
            }
        }
        Ok(())
    }
}

fn builds_docker(step: &RawStep) -> bool {
    step.uses.as_deref().and_then(Action::parse) == Some(Action::DockerBuildPush)
}

fn docker_sources(step: &RawStep) -> impl Iterator<Item = (String, &String)> {
    step.with
        .iter()
        .map(|(key, value)| (key.to_ascii_lowercase(), value))
        .filter(|(key, _)| DOCKER_SOURCE_KEYS.contains(&key.as_str()))
}

fn leaves_the_tree(value: &str) -> bool {
    let value = value.trim();
    let local = value.strip_prefix(WORKSPACE).unwrap_or(value);
    value.contains("://")
        || value.starts_with("git@")
        || value.starts_with("github.com")
        || local.starts_with('/')
        || local.starts_with("${{")
        || local.split('/').any(|part| part == "..")
}

fn contexts_leaving_the_tree(value: &str) -> bool {
    value
        .split(['\n', ','])
        .filter_map(|entry| entry.split_once('=').map(|(_, source)| source.trim()))
        .filter(|source| !source.starts_with("docker-image://"))
        .any(leaves_the_tree)
}

fn docker_outside_tree(step: &RawStep) -> Option<&str> {
    step.with.iter().find_map(|(key, value)| {
        let outside = if key.eq_ignore_ascii_case(DOCKER_CONTEXTS_KEY) {
            contexts_leaving_the_tree(value)
        } else {
            DOCKER_PATH_KEYS
                .iter()
                .any(|name| key.eq_ignore_ascii_case(name))
                && leaves_the_tree(value)
        };
        outside.then_some(key.as_str())
    })
}

fn lock_platform(label: &str) -> Option<String> {
    MisePlatform::runner(label).map(MisePlatform::lock_key)
}

fn fence(file: &str, label: &str, raw: &RawWorkflow) -> Result<(), WorkflowsError> {
    let mut dispatch = raw
        .on
        .iter()
        .filter_map(|on| on.workflow_dispatch.as_ref())
        .flat_map(|dispatch| &dispatch.input);
    if let Some(input) = dispatch.find(|input| {
        REF_INPUTS
            .iter()
            .any(|name| input.name.eq_ignore_ascii_case(name))
    }) {
        return refuse(format!(
            "the workflow {file} sets trusted_runner, but its workflow_dispatch input {} can choose what the job builds",
            input.name
        ));
    }
    if names_label(raw, label) {
        return refuse(format!(
            "the workflow {file} sets trusted_runner {label}, but a job or matrix entry names that label itself"
        ));
    }
    let steps: Vec<&crate::raw::RawStep> = raw
        .job
        .iter()
        .flat_map(|job| &job.step)
        .chain(raw.case.iter().flat_map(|case| &case.step))
        .collect();
    if steps
        .iter()
        .flat_map(|step| step.with.keys())
        .any(|key| key.eq_ignore_ascii_case(SERVER_KEY))
    {
        return refuse(format!(
            "the workflow {file} sets trusted_runner, but a step sets {SERVER_KEY}, which chooses the server the job checks out from"
        ));
    }
    let checkout: Vec<&String> = steps
        .iter()
        .flat_map(|step| &step.with)
        .filter(|(key, _)| {
            CHECKOUT_KEYS
                .iter()
                .any(|name| key.eq_ignore_ascii_case(name))
        })
        .map(|(_, value)| value)
        .collect();
    if checkout.iter().any(|value| reads_matrix(value)) {
        return refuse(format!(
            "the workflow {file} sets trusted_runner, but a step reads the ref or repository to check out from the matrix"
        ));
    }
    if checkout.iter().any(|value| reads_input(value)) {
        return refuse(format!(
            "the workflow {file} sets trusted_runner, but a step reads the ref or repository to check out from an input"
        ));
    }
    if checkout.iter().any(|value| reads_step_or_job(value)) {
        return refuse(format!(
            "the workflow {file} sets trusted_runner, but a step reads the ref or repository to check out from a step or job output"
        ));
    }
    let docker: Vec<&RawStep> = steps
        .iter()
        .copied()
        .filter(|step| builds_docker(step))
        .collect();
    if docker
        .iter()
        .flat_map(|step| docker_sources(step))
        .any(|(_, value)| reads_input(value) || reads_matrix(value) || reads_step_or_job(value))
    {
        return refuse(format!(
            "the workflow {file} sets trusted_runner, but a Docker build step reads its context, Dockerfile, build contexts or build arguments from an input, the matrix or a step or job output"
        ));
    }
    if let Some(key) = docker.iter().find_map(|step| docker_outside_tree(step)) {
        return refuse(format!(
            "the workflow {file} sets trusted_runner, but a Docker build step sets {key} to a source outside the checked out tree"
        ));
    }
    Ok(())
}

pub(crate) fn pool(
    file: &str,
    runner: &str,
    fork_runner: &str,
    hosted: bool,
    raw: &RawWorkflow,
) -> Result<Option<TrustedPool>, WorkflowsError> {
    let Some(label) = raw.trusted_runner.as_deref() else {
        return Ok(None);
    };
    let label = runner_label_of(file, label).or_else(|_| {
        refuse(format!(
            "the trusted_runner {label:?} of {file} must be a label of letters, digits, dashes, underscores and dots"
        ))
    })?;
    let has_cases = !raw.case.is_empty();
    let replaces = match raw.scale_set_runner.as_deref() {
        Some(scale_set) if has_cases => scale_set,
        _ => runner,
    };
    let differs = |other: &str| !label.eq_ignore_ascii_case(other);
    let others = [
        Some(runner),
        Some(replaces),
        Some(fork_runner),
        raw.hosted_runner.as_deref().filter(|_| has_cases),
    ];
    if looks_github_hosted(&label)
        || looks_github_hosted(replaces)
        || (!has_cases && hosted)
        || !others.into_iter().flatten().all(differs)
        || lock_platform(&label) != lock_platform(replaces)
    {
        return refuse(format!(
            "the trusted_runner {label} of {file} must be a self-hosted label that differs from the runner, fork_runner and hosted_runner, shares the platform of the self-hosted runner {replaces}, and needs a runner that is not GitHub-hosted"
        ));
    }
    fence(file, &label, raw)?;
    Ok(Some(TrustedPool {
        label,
        replaces: replaces.to_owned(),
    }))
}
