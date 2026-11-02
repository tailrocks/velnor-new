// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    HomeKey, Job, RUSTUP_AUTO_INSTALL_DISABLED, RUSTUP_AUTO_INSTALL_ENV, Step, StepKind,
};

use crate::waste_lint::{ExecWord, WasteFinding};

fn applies(job: &Job) -> bool {
    crate::rust_job_env::is_rust_bearing(job)
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
    let StepKind::Shell { run, .. } = &step.kind else {
        return None;
    };
    let words = crate::cache_p08_detect::detector_words(run);
    let names_rust = words
        .iter()
        .any(|word| velnor_actions_mise::rust_spec_version(word).is_some());
    words
        .iter()
        .filter_map(|word| ExecWord::from_word(word))
        .filter(|word| *word != ExecWord::Mise || names_rust)
        .min()
}

fn home_findings(id: &str, step: &Step, word: ExecWord) -> Vec<WasteFinding> {
    let env = step_env(step).cloned().unwrap_or_default();
    let missing: Vec<HomeKey> = HomeKey::ALL
        .into_iter()
        .filter(|key| !env.contains_key(key.env_name()))
        .collect();
    let mut findings = Vec::new();
    if !missing.is_empty() {
        findings.push(WasteFinding::MissingHome {
            job: id.to_owned(),
            step: step.name.clone(),
            word,
            missing,
        });
    }
    for key in HomeKey::ALL {
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
        .filter(|(_, job)| applies(job))
        .flat_map(|(id, job)| {
            job.steps.iter().flat_map(move |step| {
                exec_word(step)
                    .map(|word| home_findings(id, step, word))
                    .unwrap_or_default()
            })
        })
        .collect()
}

pub(crate) fn w3(jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
    let mut findings = Vec::new();
    for (id, job) in jobs {
        if applies(job)
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
