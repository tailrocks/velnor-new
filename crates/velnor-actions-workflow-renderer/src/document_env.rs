// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::Job;
use velnor_actions_contract::workflow::cache_family::{
    MBX_BSE_CONSUMER, MBX_BSE_ENV, MBX_BSE_PRODUCER,
};
use velnor_actions_contract::workflow::{ProducerKind, WRITER_MISE_ENV};

use crate::cache_steps::{
    MBX_COMPILE_ENV, MBX_EXPORT_GROUP_ENV, MBX_EXPORT_GROUP_VALUE, MBX_GC_AUTO_ENV,
    MBX_GC_AUTO_VALUE,
};
use crate::render::{PLAN_ATTEMPT_ENV, PLAN_JOB_ID};

pub(super) fn plan_attempt_value(
    id: &str,
    job: &Job,
    jobs: &BTreeMap<String, Job>,
) -> Option<&'static str> {
    if !jobs.contains_key(PLAN_JOB_ID) {
        return None;
    }
    if id != PLAN_JOB_ID && job.needs.iter().any(|need| need == PLAN_JOB_ID) {
        Some("${{ needs.plan.outputs.plan_attempt || github.run_attempt }}")
    } else {
        Some("${{ github.run_attempt }}")
    }
}

const MBX_SUMMARY_ENV: &str = "MBX_SUMMARY";
const MBX_SUMMARY_VALUE: &str = "ci";

pub(crate) fn mbx_job_env(build_script_execution: &str) -> BTreeMap<String, String> {
    [
        (MBX_GC_AUTO_ENV, MBX_GC_AUTO_VALUE),
        (MBX_EXPORT_GROUP_ENV, MBX_EXPORT_GROUP_VALUE),
        (MBX_BSE_ENV, build_script_execution),
        (MBX_SUMMARY_ENV, MBX_SUMMARY_VALUE),
    ]
    .into_iter()
    .chain(MBX_COMPILE_ENV)
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

pub(super) fn extend_job_env(
    job_env: &mut BTreeMap<String, String>,
    job: &Job,
    native_mbx: bool,
    plan_attempt: Option<&str>,
) {
    if native_mbx {
        let producer = job.producer_kind() == Some(ProducerKind::StateProducer);
        job_env.extend(mbx_job_env(if producer {
            MBX_BSE_PRODUCER
        } else {
            MBX_BSE_CONSUMER
        }));
    }
    if let Some(value) = plan_attempt {
        job_env.insert(PLAN_ATTEMPT_ENV.to_owned(), value.to_owned());
    }
}

pub(super) fn extend_writer_env(job_env: &mut BTreeMap<String, String>, job: &Job) {
    if job.cache_access.is_read() {
        return;
    }
    job_env.extend(
        WRITER_MISE_ENV
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
}

pub(super) fn workflow_env(
    verification_tasks_present: bool,
    mise_quiet: bool,
) -> BTreeMap<String, String> {
    let mut env = crate::toolchain_env::credential_scrub();
    if !verification_tasks_present {
        env.extend(
            crate::toolchain_env::MISE_STATIC_ENV
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value.to_owned())),
        );
        if mise_quiet {
            env.insert("MISE_QUIET".to_owned(), "1".to_owned());
        }
    }
    env
}

pub(super) fn exclude_inherited(
    job: &BTreeMap<String, String>,
    workflow: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    job.iter()
        .filter(|(key, value)| workflow.get(*key) != Some(*value))
        .map(|(key, value)| ((*key).clone(), (*value).clone()))
        .collect()
}
