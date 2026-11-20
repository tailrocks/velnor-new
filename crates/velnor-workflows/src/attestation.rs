// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::job_limits::runs_script;
use crate::plan::{Action, Body, Job, Step, WorkflowsPlan};
use crate::wrappers::{Invoked, invoked};

const PROVENANCE_SCOPES: [&str; 2] = ["id-token", "attestations"];

impl WorkflowsPlan {
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        self.workflows
            .iter()
            .flat_map(|workflow| {
                workflow
                    .jobs
                    .iter()
                    .filter_map(|job| provenance_warning(&workflow.file, job))
            })
            .collect()
    }
}

fn provenance_warning(file: &str, job: &Job) -> Option<String> {
    let scopes: Vec<&str> = job
        .permissions
        .iter()
        .filter(|(scope, level)| level == "write" && PROVENANCE_SCOPES.contains(&scope.as_str()))
        .map(|(scope, _)| scope.as_str())
        .collect();
    if scopes.is_empty() {
        return None;
    }
    let attest = job
        .steps
        .iter()
        .position(|step| matches!(step.body, Body::Uses(Action::AttestBuildProvenance)))?;
    let step = job.steps[..attest]
        .iter()
        .find(|step| runs_repository_code(step))?;
    let grants: Vec<String> = scopes
        .iter()
        .map(|scope| format!("{scope}: write"))
        .collect();
    Some(format!(
        "warning: workflow {file}.yml job {} grants {} and runs repository code in step {:?} before its attest step; split the build and the attest into separate jobs, like the sums-family job of release.yml: the build job keeps contents: read and uploads the artifact, and the attest job holds id-token and attestations write, downloads the artifact and runs no repository code",
        job.id,
        grants.join(" and "),
        step.name,
    ))
}

fn runs_repository_code(step: &Step) -> bool {
    match &step.body {
        Body::Run(run) => runs_script(run) || runs_cargo(run),
        Body::Local(_) => true,
        Body::Uses(_) | Body::Helper(_) => false,
    }
}

fn runs_cargo(run: &str) -> bool {
    invoked(run).iter().any(|call| match call {
        Invoked::Command(words) => words
            .first()
            .is_some_and(|word| word.rsplit('/').next() == Some("cargo")),
        Invoked::Opaque(_) => false,
    })
}
