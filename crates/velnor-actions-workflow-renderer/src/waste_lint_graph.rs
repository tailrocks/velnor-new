// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::cache_family::{
    SOURCES_KEY_PLAN_EXPR, SOURCES_KEY_STEP_EXPR,
};
use velnor_actions_contract::{
    HOSTED_SUFFIX, Job, PLAN_JOB_ID, SCALE_SUFFIX, Step, StepKind, StepRole,
};

use crate::cache_doctor::cache_ops;
use crate::dispatch_cache_boundary::SAVE_DENY;
use crate::waste_lint::WasteFinding;

const OUTPUT_ROLES: [StepRole; 6] = [
    StepRole::ToolsCacheRestore,
    StepRole::CargoSourcesRestore,
    StepRole::MbxBundleKey,
    StepRole::MbxBundleRestore,
    StepRole::FamilyRestore,
    StepRole::TofuProvidersRestore,
];

type Owners<'a> = (BTreeSet<&'a str>, BTreeSet<&'a str>);

fn lane_base(id: &str) -> &str {
    id.strip_suffix(HOSTED_SUFFIX)
        .or_else(|| id.strip_suffix(SCALE_SUFFIX))
        .unwrap_or(id)
}

pub(crate) fn w1(jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
    let mut owners: BTreeMap<&str, Owners<'_>> = BTreeMap::new();
    for (id, job) in jobs {
        let producer = job.producer_kind().is_some();
        for step in &job.steps {
            if let StepKind::TaskExecution { task_digest, .. } = &step.kind {
                let (crates, producers) = owners.entry(task_digest).or_default();
                if producer { producers } else { crates }.insert(lane_base(id));
            }
        }
    }
    owners
        .into_iter()
        .filter_map(|(digest, (crates, producers))| {
            let mut repeated: Vec<String> = [crates, producers]
                .into_iter()
                .filter(|owners| owners.len() > 1)
                .flatten()
                .map(|id| (*id).to_owned())
                .collect();
            repeated.sort();
            (!repeated.is_empty()).then(|| WasteFinding::DuplicateTask {
                digest: digest.to_owned(),
                jobs: repeated,
            })
        })
        .collect()
}

fn canonical_key(key: &str) -> &str {
    if key == SOURCES_KEY_STEP_EXPR {
        SOURCES_KEY_PLAN_EXPR
    } else {
        key
    }
}

pub(crate) fn w4(jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
    let mut found = Vec::new();
    for (id, job) in jobs {
        let mut seen = BTreeSet::new();
        for op in cache_ops(id, job).filter(|op| !op.save) {
            let key = canonical_key(op.key);
            if !seen.insert(key) {
                found.push(WasteFinding::DuplicateRestore {
                    job: id.clone(),
                    key: key.to_owned(),
                });
            }
        }
    }
    found
}

pub(crate) fn w5(jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
    jobs.iter()
        .filter(|(_, job)| job.producer_kind().is_none())
        .flat_map(|(id, job)| {
            cache_ops(id, job)
                .filter(|op| op.save && op.step.condition.as_deref() != Some(SAVE_DENY))
                .map(move |op| WasteFinding::NonProducerSave {
                    job: id.clone(),
                    step: op.step.name.clone(),
                })
        })
        .collect()
}

pub(crate) fn w6(jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
    jobs.iter()
        .filter(|(id, _)| id.as_str() != PLAN_JOB_ID)
        .flat_map(|(id, job)| {
            let sources: Vec<String> = job
                .steps
                .iter()
                .filter(|step| step.role.is_some_and(|role| OUTPUT_ROLES.contains(&role)))
                .filter_map(|step| step.id)
                .map(|step| format!("steps.{}.outputs.", step.as_str()))
                .collect();
            job.outputs
                .iter()
                .filter(move |(_, value)| sources.iter().any(|source| value.contains(source)))
                .filter(|(name, _)| !consumed(jobs, id, name))
                .map(|(name, _)| WasteFinding::UnconsumedOutput {
                    job: id.clone(),
                    output: name.clone(),
                })
        })
        .collect()
}

fn consumed(jobs: &BTreeMap<String, Job>, producer: &str, output: &str) -> bool {
    let needle = format!("needs.{producer}.outputs.{output}");
    jobs.iter()
        .filter(|(id, _)| id.as_str() != producer)
        .flat_map(|(_, job)| job_texts(job))
        .any(|text| reads(text, &needle))
}

fn reads(text: &str, needle: &str) -> bool {
    text.match_indices(needle).any(|(at, _)| {
        !text[at + needle.len()..]
            .starts_with(|next: char| next.is_ascii_alphanumeric() || next == '_' || next == '-')
    })
}

fn job_texts(job: &Job) -> Vec<&str> {
    let mut texts: Vec<&str> = job.condition.iter().map(String::as_str).collect();
    texts.push(&job.runs_on);
    texts.extend(job.outputs.values().map(String::as_str));
    for step in &job.steps {
        step_texts(step, &mut texts);
    }
    texts
}

fn step_texts<'a>(step: &'a Step, texts: &mut Vec<&'a str>) {
    texts.extend(step.condition.iter().map(String::as_str));
    match &step.kind {
        StepKind::Action { with, env, .. } => {
            texts.extend(with.values().chain(env.values()).map(String::as_str));
        }
        StepKind::Shell { run, env } => {
            texts.extend(run.iter().map(String::as_str));
            texts.extend(env.values().map(String::as_str));
        }
        StepKind::TaskExecution { argv, env, .. } => {
            texts.extend(argv.iter().map(String::as_str));
            texts.extend(env.values().map(String::as_str));
        }
        StepKind::Internal { env, .. } => texts.extend(env.values().map(String::as_str)),
        StepKind::NativeCheckExecution { .. } => {}
    }
}
