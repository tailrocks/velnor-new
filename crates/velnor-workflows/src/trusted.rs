// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{MisePlatform, looks_github_hosted};

use crate::check::runner_label_of;
use crate::error::{WorkflowsError, refuse};
use crate::plan::TrustedPool;
use crate::raw::RawWorkflow;

const REF_INPUTS: [&str; 3] = ["ref", "sha", "repository"];
const CHECKOUT_KEYS: [&str; 2] = ["ref", "repository"];

fn reads_input(value: &str) -> bool {
    value.contains("inputs.") || value.contains("github.event.inputs")
}

fn reads_step_or_job(value: &str) -> bool {
    value.contains("steps.") || value.contains("needs.")
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
    let steps = raw
        .job
        .iter()
        .flat_map(|job| &job.step)
        .chain(raw.case.iter().flat_map(|case| &case.step));
    let checkout: Vec<&String> = steps
        .flat_map(|step| &step.with)
        .filter(|(key, _)| CHECKOUT_KEYS.contains(&key.as_str()))
        .map(|(_, value)| value)
        .collect();
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
