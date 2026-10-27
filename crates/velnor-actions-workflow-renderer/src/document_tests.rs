// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::{CacheMode, Job, JobTimeout};

fn writer_job(condition: Option<&str>, cache_mode: Option<CacheMode>) -> Job {
    Job {
        display_name: "Writer".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: condition.map(str::to_owned),
        permissions: None,
        cache_mode,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        steps: Vec::new(),
    }
}

fn rendered_options(job: &Job) -> Vec<(String, Yaml)> {
    let mut entries = Vec::new();
    append_job_options(&mut entries, job, false, &BTreeMap::new(), &BTreeMap::new())
        .expect("options render");
    entries
}

fn cache_mode_entry(entries: &[(String, Yaml)]) -> Option<Yaml> {
    entries
        .iter()
        .find(|(key, _)| key == "cache-mode")
        .map(|(_, value)| value.clone())
}

#[test]
fn push_gated_writer_renders_literal_write() {
    let gated = writer_job(
        Some("(affected) && github.event_name == 'push'"),
        Some(CacheMode::push_write_expression()),
    );
    assert_eq!(
        cache_mode_entry(&rendered_options(&gated)),
        Some(Yaml::Str("write".to_owned()))
    );
    let bare = writer_job(
        Some("github.event_name == 'push'"),
        Some(CacheMode::push_write_expression()),
    );
    assert_eq!(
        cache_mode_entry(&rendered_options(&bare)),
        Some(Yaml::Str("write".to_owned()))
    );
}

#[test]
fn unprovable_writer_keeps_no_declaration() {
    let unconditioned = writer_job(None, Some(CacheMode::push_write_expression()));
    assert_eq!(cache_mode_entry(&rendered_options(&unconditioned)), None);
    let pr_capable = writer_job(Some("(affected)"), Some(CacheMode::push_write_expression()));
    assert_eq!(cache_mode_entry(&rendered_options(&pr_capable)), None);
    let or_shaped = writer_job(
        Some("(reader_ok) || (writer_ok) && github.event_name == 'push'"),
        Some(CacheMode::push_write_expression()),
    );
    assert_eq!(cache_mode_entry(&rendered_options(&or_shaped)), None);
    let narrowed = writer_job(
        Some("((reader_ok) || (writer_ok)) && github.event_name == 'push'"),
        Some(CacheMode::push_write_expression()),
    );
    assert_eq!(
        cache_mode_entry(&rendered_options(&narrowed)),
        Some(Yaml::Str("write".to_owned()))
    );
}

#[test]
fn explicit_modes_render_unchanged() {
    let reader = writer_job(Some("(affected)"), Some(CacheMode::explicit_read()));
    assert_eq!(
        cache_mode_entry(&rendered_options(&reader)),
        Some(Yaml::Str("read".to_owned()))
    );
}
