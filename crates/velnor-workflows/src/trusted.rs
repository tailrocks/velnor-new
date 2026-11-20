// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{MisePlatform, looks_github_hosted};

use crate::check::runner_label_of;
use crate::error::{WorkflowsError, refuse};
use crate::plan::{Input, TrustedPool};
use crate::raw::RawStep;

const REF_INPUTS: [&str; 3] = ["ref", "sha", "repository"];
const CHECKOUT_KEYS: [&str; 2] = ["ref", "repository"];

pub(crate) struct Declared<'a> {
    pub(crate) file: &'a str,
    pub(crate) label: Option<&'a str>,
    pub(crate) runner: &'a str,
    pub(crate) hosted: bool,
    pub(crate) fork_runner: &'a str,
    pub(crate) cases: Option<CasePair<'a>>,
}

pub(crate) struct CasePair<'a> {
    pub(crate) hosted_runner: Option<&'a str>,
    pub(crate) scale_set_runner: Option<&'a str>,
}

fn lock_platform(label: &str) -> Option<String> {
    MisePlatform::runner(label).map(MisePlatform::lock_key)
}

pub(crate) fn pool<'a>(
    declared: &Declared<'_>,
    dispatch: &[Input],
    steps: impl Iterator<Item = &'a RawStep>,
) -> Result<Option<TrustedPool>, WorkflowsError> {
    let Some(label) = declared.label else {
        return Ok(None);
    };
    let file = declared.file;
    let label = runner_label_of(file, label).or_else(|_| {
        refuse(format!(
            "the trusted_runner {label:?} of {file} must be a label of letters, digits, dashes, underscores and dots"
        ))
    })?;
    let replaces = match &declared.cases {
        Some(pair) => pair.scale_set_runner.unwrap_or(declared.runner),
        None => declared.runner,
    };
    let differs = |other: &str| !label.eq_ignore_ascii_case(other);
    let others = [
        Some(replaces),
        Some(declared.fork_runner),
        declared.cases.as_ref().and_then(|pair| pair.hosted_runner),
    ];
    if looks_github_hosted(&label)
        || looks_github_hosted(replaces)
        || (declared.cases.is_none() && declared.hosted)
        || !others.into_iter().flatten().all(differs)
        || lock_platform(&label) != lock_platform(replaces)
    {
        return refuse(format!(
            "the trusted_runner {label} of {file} must be a self-hosted label that differs from the runner, fork_runner and hosted_runner, shares the platform of the self-hosted runner {replaces}, and needs a runner that is not GitHub-hosted"
        ));
    }
    if let Some(input) = dispatch.iter().find(|input| {
        REF_INPUTS
            .iter()
            .any(|name| input.name.eq_ignore_ascii_case(name))
    }) {
        return refuse(format!(
            "the workflow {file} sets trusted_runner, but its workflow_dispatch input {} can choose what the job builds",
            input.name
        ));
    }
    let chosen = steps.flat_map(|step| &step.with).any(|(key, value)| {
        CHECKOUT_KEYS.contains(&key.as_str())
            && (value.contains("inputs.") || value.contains("github.event.inputs"))
    });
    if chosen {
        return refuse(format!(
            "the workflow {file} sets trusted_runner, but a step reads the ref or repository to check out from an input"
        ));
    }
    Ok(Some(TrustedPool {
        label,
        replaces: replaces.to_owned(),
    }))
}
