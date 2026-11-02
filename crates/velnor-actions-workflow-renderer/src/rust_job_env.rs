// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobEnv};

pub(crate) fn is_rust_bearing(job: &Job) -> bool {
    crate::cache_p08::infer_job_tools(job)
        .iter()
        .any(|spec| velnor_actions_mise::rust_spec_version(spec).is_some())
}

pub(crate) fn bind(jobs: &mut BTreeMap<String, Job>) {
    for job in jobs.values_mut() {
        if is_rust_bearing(job) {
            job.env = JobEnv::RustToolchain;
        }
    }
}

pub(crate) fn extend_env(env: &mut BTreeMap<String, String>, job: &Job) {
    env.extend(
        job.env
            .pairs()
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned())),
    );
}
