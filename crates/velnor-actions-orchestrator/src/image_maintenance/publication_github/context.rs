// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{get_json, models::{Artifact, Job, Run}, pagination};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_mise::CheckDeadline;

pub(in crate::image_maintenance) fn authenticate_publication_context_run(
    source: &str, run: u64, attempt: u32, token: &str, deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let route = format!("repos/tailrocks/velnor-new/actions/runs/{run}/attempts/{attempt}");
    let current: Run = get_json(&route, token, deadline)?;
    if current.id != run || current.attempt != attempt || current.head_sha != source
        || current.event != "push" || current.path != ".github/workflows/product-release-images.yml"
        || current.head_branch != "main" || current.head_repository.as_ref().is_none_or(|repo|
            repo.id != 1_390_620_900 || repo.full_name != "tailrocks/velnor-new") {
        return Err(internal("mac_context_run_identity"));
    }
    let route = format!("repos/tailrocks/velnor-new/actions/runs/{run}/attempts/{attempt}/jobs");
    let jobs: Vec<Job> = pagination::get_complete(&pagination::Collection {
        route: &route, name: "jobs", max_items: 1000,
        error: "mac_context_job_inventory", identity_error: "mac_context_job_identity", id: |job: &Job| job.id,
    }, token, deadline)?;
    let publisher: Vec<_> = jobs.iter().filter(|job| job.name == "publish-images").collect();
    if publisher.len() != 1 || publisher[0].run_id != Some(run)
        || publisher[0].run_attempt != Some(attempt) || publisher[0].head_sha.as_deref() != Some(source)
        || publisher[0].status != "completed" || publisher[0].conclusion.as_deref() != Some("success") {
        return Err(internal("mac_context_publisher_identity"));
    }
    Ok(())
}

pub(in crate::image_maintenance) fn context_artifact_id(
    run: u64, attempt: u32, token: &str, deadline: CheckDeadline,
) -> Result<u64, OrchestratorError> {
    let route = format!("repos/tailrocks/velnor-new/actions/runs/{run}/artifacts");
    let artifacts: Vec<Artifact> = pagination::get_complete(&pagination::Collection {
        route: &route, name: "artifacts", max_items: 2000,
        error: "mac_context_artifact_inventory", identity_error: "mac_context_artifact_identity", id: |artifact: &Artifact| Some(artifact.id),
    }, token, deadline)?;
    let name = format!("runner-image-publication-context-{attempt}");
    let mut matches = artifacts.iter().filter(|artifact| artifact.name == name);
    let artifact = matches.next().ok_or_else(|| internal("mac_context_artifact_count"))?;
    if matches.next().is_some() { return Err(internal("mac_context_artifact_count")); }
    Ok(artifact.id)
}
