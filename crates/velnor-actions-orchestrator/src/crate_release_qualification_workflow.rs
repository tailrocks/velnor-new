// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_github::GitHubAdmissionRead;
use crate::crate_release_qualification_witness::WitnessRequest;
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::collections::BTreeSet;
use velnor_actions_contract::config::ReleaseRehearsalConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalWorkflowEvidence {
    pub workflow_sha256: String,
    pub tree_sha: String,
}

pub fn verify_canonical_rehearsal(
    request: &WitnessRequest,
    default_branch: &str,
    api: &dyn GitHubAdmissionRead,
) -> Result<CanonicalWorkflowEvidence, OrchestratorError> {
    let invalid = || internal("qualification_workflow_invalid");
    if !velnor_actions_contract::vcs::is_safe_github_repository(&request.repository)
        || request.repository_id == 0
        || request.run_id == 0
        || request.run_attempt != 1
        || request.workflow_path != ".github/workflows/release-rehearsal.yml"
        || request.workflow_sha.len() != 40
        || !request
            .workflow_sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || default_branch.is_empty()
    {
        return Err(invalid());
    }
    let config = ReleaseRehearsalConfig {
        script: request.script.clone(),
        environment: request.environment.clone(),
        reviewer_id: request.reviewer_id,
    };
    let expected = velnor_actions_workflow_renderer::release_tree::rehearsal::render(
        &config,
        env!("CARGO_PKG_VERSION"),
    )?;
    let prefix = format!("repos/{}", request.repository);
    let repository = api.get(&prefix)?;
    if repository["id"].as_u64() != Some(request.repository_id)
        || repository["full_name"].as_str() != Some(&request.repository)
        || repository["default_branch"].as_str() != Some(default_branch)
    {
        return Err(invalid());
    }
    let commit_path = format!("{prefix}/git/commits/{}", request.workflow_sha);
    let commit = api.get(&commit_path)?;
    let tree_sha = commit["tree"]["sha"].as_str().ok_or_else(invalid)?;
    if commit["sha"].as_str() != Some(&request.workflow_sha) {
        return Err(invalid());
    }
    let trees = crate::crate_release_pr_tree::GitHubTrees {
        api,
        repository: &request.repository,
    };
    let bytes = crate::crate_release_pr_acquisition::read_reviewed_file(
        api,
        &trees,
        tree_sha,
        &request.workflow_path,
    )?;
    if bytes != expected.bytes.as_bytes() {
        return Err(invalid());
    }
    let jobs_path = format!(
        "{prefix}/actions/runs/{}/attempts/1/jobs?per_page=100&page=1",
        request.run_id
    );
    let jobs = api.get(&jobs_path)?;
    verify_jobs(&jobs, request)?;
    if api.get(&commit_path)? != commit
        || api.get(&jobs_path)? != jobs
        || api.get(&prefix)? != repository
    {
        return Err(internal("qualification_workflow_changed"));
    }
    Ok(CanonicalWorkflowEvidence {
        workflow_sha256: crate::generator::sha256_hex(&bytes),
        tree_sha: tree_sha.to_owned(),
    })
}

fn verify_jobs(value: &Value, request: &WitnessRequest) -> Result<(), OrchestratorError> {
    let invalid = || internal("qualification_workflow_jobs");
    let jobs = value["jobs"].as_array().ok_or_else(invalid)?;
    if jobs.len() != 4 || value["total_count"].as_u64() != Some(4) {
        return Err(invalid());
    }
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for job in jobs {
        let name = job["name"].as_str().ok_or_else(invalid)?;
        let id = job["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(invalid)?;
        if !matches!(name, "preflight" | "gate" | "execute" | "witness")
            || !names.insert(name)
            || !ids.insert(id)
            || job["run_id"].as_u64() != Some(request.run_id)
            || job["run_attempt"].as_u64() != Some(1)
            || job["head_sha"].as_str() != Some(&request.workflow_sha)
            || job["status"] != "completed"
            || job["conclusion"] != "success"
            || (name == "witness" && job["id"].as_u64() != Some(request.job_id))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "crate_release_qualification_workflow_tests.rs"]
mod tests;
