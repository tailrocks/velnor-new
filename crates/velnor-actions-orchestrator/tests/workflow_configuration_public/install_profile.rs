// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::{Job, StepKind, StepRole};
use velnor_actions_mise::PinnedTool;
use velnor_actions_orchestrator::{GenerationPreparation, finalized_jobs, prepare};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

use super::maintenance_features::owner_feature_repo;
use super::tools_producers::{POLICIES, is_tools_producer};
use super::tools_producers_donors::SCHEDULED_SUITES_CONFIG;
use crate::workflow_configuration_fixture::make_repo;

fn prepare_tokens(job: &Job) -> Vec<&str> {
    job.steps
        .iter()
        .filter(|step| step.role == Some(StepRole::PreparePinnedTools))
        .filter_map(|step| match &step.kind {
            StepKind::Shell { run, .. } => Some(run.iter().map(String::as_str)),
            _ => None,
        })
        .flatten()
        .collect()
}

fn rust_token(token: &str) -> bool {
    token.starts_with("rust@") || token.starts_with("rust[")
}

fn rust_install_profile(job: &Job) -> Option<String> {
    prepare_tokens(job)
        .into_iter()
        .filter(|token| rust_token(token))
        .find_map(|token| {
            let (options, _) = token.strip_prefix("rust[")?.split_once("]@")?;
            options.strip_prefix("profile=").map(str::to_owned)
        })
}

type RustJobs = (BTreeMap<String, Job>, BTreeSet<String>);

fn checked_rust_jobs(
    label: &str,
    prepared: &GenerationPreparation,
) -> Result<RustJobs, Box<dyn Error>> {
    let jobs = finalized_jobs(prepared)?;
    let expected = format!(
        "rust[profile=minimal]@{}",
        prepared.emission_catalog().version(PinnedTool::Rust)
    );
    let mut with_rust = BTreeSet::new();
    for (id, job) in &jobs {
        for token in prepare_tokens(job) {
            if rust_token(token) {
                assert_eq!(token, expected, "{label} {id}");
                assert_eq!(
                    rust_install_profile(job).as_deref(),
                    Some("minimal"),
                    "{label} {id}"
                );
                with_rust.insert(id.clone());
            } else {
                assert!(
                    !token.contains('[') || token.starts_with("http:"),
                    "{label} {id} adds an install option to {token}"
                );
            }
        }
    }
    Ok((jobs, with_rust))
}

fn producer_installs_rust(jobs: &BTreeMap<String, Job>, with_rust: &BTreeSet<String>) -> bool {
    jobs.iter()
        .any(|(id, job)| is_tools_producer(job) && with_rust.contains(id))
}

#[test]
fn every_generated_rust_install_requests_the_minimal_profile() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let repo = owner_feature_repo(policy, false, "default", "default")?;
        let (jobs, with_rust) = checked_rust_jobs(&format!("{policy:?}"), &prepare(repo.path())?)?;
        assert!(
            with_rust.contains(PLAN_JOB_ID),
            "{policy:?}: plan installs no Rust"
        );
        assert!(
            producer_installs_rust(&jobs, &with_rust),
            "{policy:?}: no tools producer installs Rust"
        );
        assert!(
            with_rust.iter().any(|id| id.starts_with("rust-")),
            "{policy:?}: no Rust owner job installs Rust"
        );
    }
    let repo = make_repo(SCHEDULED_SUITES_CONFIG)?;
    let (jobs, with_rust) = checked_rust_jobs("scheduled suites", &prepare(repo.path())?)?;
    assert!(
        producer_installs_rust(&jobs, &with_rust),
        "no suite tools producer installs Rust"
    );
    Ok(())
}
