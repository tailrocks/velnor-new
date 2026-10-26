// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::Job;

use crate::cache_steps::{
    MBX_BUILD_SCRIPT_EXECUTION_ENV, MBX_BUILD_SCRIPT_EXECUTION_VALUE, MBX_CC_ENV, MBX_CC_VALUE,
    MBX_EXPORT_GROUP_ENV, MBX_EXPORT_GROUP_VALUE, MBX_GC_AUTO_ENV, MBX_GC_AUTO_VALUE,
    MBX_SHARE_OUT_DIR_ENV, MBX_SHARE_OUT_DIR_VALUE,
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

pub(super) fn extend_job_env(
    job_env: &mut BTreeMap<String, String>,
    native_mbx: bool,
    plan_attempt: Option<&str>,
) {
    if native_mbx {
        for (key, val) in [
            (MBX_GC_AUTO_ENV, MBX_GC_AUTO_VALUE),
            (MBX_EXPORT_GROUP_ENV, MBX_EXPORT_GROUP_VALUE),
            (MBX_SHARE_OUT_DIR_ENV, MBX_SHARE_OUT_DIR_VALUE),
            (MBX_CC_ENV, MBX_CC_VALUE),
            (
                MBX_BUILD_SCRIPT_EXECUTION_ENV,
                MBX_BUILD_SCRIPT_EXECUTION_VALUE,
            ),
        ] {
            job_env.insert(key.to_owned(), val.to_owned());
        }
    }
    if let Some(value) = plan_attempt {
        job_env.insert(PLAN_ATTEMPT_ENV.to_owned(), value.to_owned());
    }
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
