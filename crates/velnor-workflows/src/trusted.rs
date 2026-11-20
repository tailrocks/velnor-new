// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{MisePlatform, looks_github_hosted};

use crate::check::runner_label_of;
use crate::error::{WorkflowsError, refuse};
use crate::plan::TrustedPool;
use crate::raw::RawWorkflow;

const REF_INPUTS: &[&str] = &["ref", "sha", "repository"];
const CHECKOUT_KEYS: &[&str] = &["ref", "repository"];
const SERVER_KEY: &str = "github-server-url";

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

fn lock_platform(label: &str) -> Option<String> {
    MisePlatform::runner(label).map(MisePlatform::lock_key)
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
    if names_label(raw, &label) {
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
    Ok(Some(TrustedPool {
        label,
        replaces: replaces.to_owned(),
    }))
}
