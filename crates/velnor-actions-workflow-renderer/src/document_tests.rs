// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::{CacheAccess, CacheWriter, Job, JobTimeout};

fn job_with(cache_access: CacheAccess) -> Job {
    Job {
        display_name: "Job".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_access,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: velnor_actions_contract::JobEnv::Unset,
        steps: Vec::new(),
    }
}

fn cache_mode_entry(job: &Job) -> Option<Yaml> {
    let mut entries = Vec::new();
    append_job_options(&mut entries, job, false, &BTreeMap::new(), &BTreeMap::new())
        .expect("options render");
    entries
        .into_iter()
        .find(|(key, _)| key == "cache-mode")
        .map(|(_, value)| value)
}

#[test]
fn only_writers_declare_a_job_cache_mode() {
    assert_eq!(cache_mode_entry(&job_with(CacheAccess::Read)), None);
    for writer in [
        CacheWriter::ToolsProducer,
        CacheWriter::TofuProvidersProducer,
        CacheWriter::SourcesProducer,
    ] {
        assert_eq!(
            cache_mode_entry(&job_with(CacheAccess::Writer(writer))),
            Some(Yaml::Str("write".to_owned()))
        );
    }
}
