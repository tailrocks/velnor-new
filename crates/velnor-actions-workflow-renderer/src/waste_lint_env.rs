// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    HomeKey, Job, RUSTUP_AUTO_INSTALL_DISABLED, RUSTUP_AUTO_INSTALL_ENV, Step, StepKind, StepRole,
};

use crate::waste_lint::{ExecWord, WasteFinding};

const VERIFIER_HOMES: [HomeKey; 2] = [HomeKey::CargoHome, HomeKey::RustupHome];

#[derive(Clone, Copy)]
enum HomeSubject {
    Exec(ExecWord),
    RustVerifier,
}

impl HomeSubject {
    fn keys(self) -> &'static [HomeKey] {
        match self {
            Self::Exec(_) => &HomeKey::ALL,
            Self::RustVerifier => &VERIFIER_HOMES,
        }
    }
}

fn is_rust_verifier(step: &Step) -> bool {
    step.role == Some(StepRole::VerifyRustManifest) && matches!(step.kind, StepKind::Shell { .. })
}

fn step_env(step: &Step) -> Option<&BTreeMap<String, String>> {
    match &step.kind {
        StepKind::Shell { env, .. }
        | StepKind::Action { env, .. }
        | StepKind::TaskExecution { env, .. } => Some(env),
        StepKind::NativeCheckExecution { .. } | StepKind::Internal { .. } => None,
    }
}

fn exec_word(step: &Step) -> Option<ExecWord> {
    let StepKind::Shell { .. } = &step.kind else {
        return None;
    };
    let tokens = crate::rust_job_env::step_tokens(step);
    let names_rust = tokens
        .iter()
        .any(|token| velnor_actions_mise::rust_spec_version(token).is_some());
    crate::rust_job_env::exec_words(&tokens)
        .into_iter()
        .filter(|word| *word != ExecWord::Mise || names_rust)
        .min()
}

fn home_findings(id: &str, step: &Step, subject: HomeSubject) -> Vec<WasteFinding> {
    let env = step_env(step).cloned().unwrap_or_default();
    let missing: Vec<HomeKey> = subject
        .keys()
        .iter()
        .copied()
        .filter(|key| !env.contains_key(key.env_name()))
        .collect();
    let mut findings = Vec::new();
    if !missing.is_empty() {
        findings.push(match subject {
            HomeSubject::Exec(word) => WasteFinding::MissingHome {
                job: id.to_owned(),
                step: step.name.clone(),
                word,
                missing,
            },
            HomeSubject::RustVerifier => WasteFinding::MissingVerifierHome {
                job: id.to_owned(),
                step: step.name.clone(),
                missing,
            },
        });
    }
    for key in subject.keys().iter().copied() {
        if let Some(step_value) = env.get(key.env_name())
            && step_value != key.standard_value()
        {
            findings.push(WasteFinding::ConflictingHome {
                job: id.to_owned(),
                step: step.name.clone(),
                key,
                expected: key.standard_value().to_owned(),
                found: step_value.clone(),
            });
        }
    }
    findings
}

pub(crate) fn w2(jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
    jobs.iter()
        .filter(|(_, job)| crate::rust_job_env::installs_rust(job))
        .flat_map(|(id, job)| {
            job.steps.iter().flat_map(move |step| {
                let subject = if is_rust_verifier(step) {
                    Some(HomeSubject::RustVerifier)
                } else {
                    exec_word(step).map(HomeSubject::Exec)
                };
                subject
                    .map(|subject| home_findings(id, step, subject))
                    .unwrap_or_default()
            })
        })
        .collect()
}

pub(crate) fn w3(jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
    let mut findings = Vec::new();
    for (id, job) in jobs {
        if crate::rust_job_env::is_rust_bearing(job)
            && job.env.get(RUSTUP_AUTO_INSTALL_ENV) != Some(RUSTUP_AUTO_INSTALL_DISABLED)
        {
            findings.push(WasteFinding::AutoInstallNotDisabled {
                job: id.clone(),
                step: None,
                found: None,
            });
        }
        for step in &job.steps {
            let found = step_env(step).and_then(|env| env.get(RUSTUP_AUTO_INSTALL_ENV));
            if let Some(value) = found
                && value != RUSTUP_AUTO_INSTALL_DISABLED
            {
                findings.push(WasteFinding::AutoInstallNotDisabled {
                    job: id.clone(),
                    step: Some(step.name.clone()),
                    found: Some(value.clone()),
                });
            }
        }
    }
    findings
}
