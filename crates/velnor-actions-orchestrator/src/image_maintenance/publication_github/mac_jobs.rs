// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::models::{Job, Run};
use crate::{OrchestratorError, internal::internal};

#[cfg(test)]
#[path = "mac_jobs_tests.rs"]
mod tests;

pub(super) fn validate_release_jobs(
    source: &str,
    run: u64,
    attempt: u32,
    jobs: &[Job],
    q_identity: Option<(&str, u64, &Run)>,
) -> Result<(), OrchestratorError> {
    let publisher = jobs
        .iter()
        .filter(|job| job.name == "publish-images")
        .collect::<Vec<_>>();
    if publisher.len() != 1
        || publisher[0].run_id != Some(run)
        || publisher[0].head_sha.as_deref() != Some(source)
        || publisher[0].status != "completed"
        || publisher[0].conclusion.as_deref() != Some("success")
    {
        return Err(internal("mac_release_job_identity"));
    }
    let matches = if let Some((sha, check_run, current)) = q_identity {
        if current.id != run
            || current.attempt != attempt
            || current.head_sha != source
            || current.event != "push"
            || current.path != ".github/workflows/product-release-images.yml"
            || current.head_branch != "main"
            || current.status != "in_progress"
            || current.conclusion.is_some()
            || current.head_repository.as_ref().is_none_or(|repo| {
                repo.id != 1_390_620_900 || repo.full_name != "tailrocks/velnor-new"
            })
        {
            return Err(internal("mac_q_workflow_identity"));
        }
        let path =
            format!("tailrocks/velnor-new/.github/workflows/source-preview-mac-proof.yml@{sha}");
        if current
            .referenced_workflows
            .iter()
            .filter(|item| {
                item.path == path
                    && item.sha == sha
                    && item.reference.as_deref().is_none_or(|value| value == sha)
            })
            .count()
            != 1
        {
            return Err(internal("mac_q_workflow_identity"));
        }
        let expected =
            format!("https://api.github.com/repos/tailrocks/velnor-new/check-runs/{check_run}");
        jobs.iter()
            .filter(|job| job.check_run_url.as_deref() == Some(expected.as_str()))
            .collect::<Vec<_>>()
    } else {
        jobs.iter()
            .filter(|job| job.name == "verify-mac-proof")
            .collect::<Vec<_>>()
    };
    if matches.len() != 1
        || matches[0].run_id != Some(run)
        || matches[0].head_sha.as_deref() != Some(source)
        || (q_identity.is_some() && matches[0].run_attempt != Some(attempt))
        || matches[0].status != "in_progress"
        || matches[0].conclusion.is_some()
    {
        return Err(internal("mac_release_job_identity"));
    }
    Ok(())
}
