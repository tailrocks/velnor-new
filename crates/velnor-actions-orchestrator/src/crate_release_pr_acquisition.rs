// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use serde_json::Value;

#[path = "crate_release_pr_workflow.rs"]
mod workflow;
pub(crate) use workflow::verify_reviewed_workflow;

#[derive(Clone, Copy)]
pub(crate) struct RunBinding<'a> {
    pub repository: &'a str,
    pub branch: &'a str,
    pub sha: &'a str,
    pub run_id: u64,
    pub attempt: u32,
}

pub(crate) fn verify_run_and_producer(
    binding: &RunBinding<'_>,
    repository: &Value,
    run: &Value,
    jobs: &Value,
    branch_ref: &Value,
) -> Result<u64, OrchestratorError> {
    let invalid = || internal("release_pr_run_unverified");
    let repo_id = repository["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(invalid)?;
    if binding.run_id == 0
        || binding.attempt != 1
        || repository["full_name"].as_str() != Some(binding.repository)
        || repository["default_branch"].as_str() != Some(binding.branch)
        || run["id"].as_u64() != Some(binding.run_id)
        || run["run_attempt"].as_u64() != Some(u64::from(binding.attempt))
        || !matches!(run["path"].as_str(), Some(path) if path == ".github/workflows/release.yml" || path == format!(".github/workflows/release.yml@{}", binding.branch))
        || run["event"].as_str() != Some("push")
        || run["status"].as_str() != Some("in_progress")
        || run["head_sha"].as_str() != Some(binding.sha)
        || run["head_branch"].as_str() != Some(binding.branch)
        || run["repository"]["id"].as_u64() != Some(repo_id)
        || run["head_repository"]["id"].as_u64() != Some(repo_id)
        || run["repository"]["full_name"].as_str() != Some(binding.repository)
        || run["head_repository"]["full_name"].as_str() != Some(binding.repository)
        || branch_ref["ref"].as_str() != Some(format!("refs/heads/{}", binding.branch).as_str())
        || branch_ref["object"]["type"].as_str() != Some("commit")
        || branch_ref["object"]["sha"].as_str() != Some(binding.sha)
    {
        return Err(invalid());
    }
    let entries = jobs["jobs"].as_array().ok_or_else(invalid)?;
    if entries.len() >= 100 || jobs["total_count"].as_u64() != Some(entries.len() as u64) {
        return Err(invalid());
    }
    let mut matched = entries
        .iter()
        .filter(|job| job["name"].as_str() == Some("Release preparation"));
    let job = matched.next().ok_or_else(invalid)?;
    if matched.next().is_some()
        || job["id"].as_u64().is_none_or(|id| id == 0)
        || job["run_id"].as_u64() != Some(binding.run_id)
        || job
            .get("run_attempt")
            .is_some_and(|attempt| attempt.as_u64() != Some(u64::from(binding.attempt)))
        || job["head_sha"].as_str() != Some(binding.sha)
        || job["status"].as_str() != Some("completed")
        || job["conclusion"].as_str() != Some("success")
    {
        return Err(invalid());
    }
    Ok(repo_id)
}

#[derive(Clone, Copy)]
pub(crate) struct AcquisitionRequest<'a> {
    pub binding: RunBinding<'a>,
    pub artifact_id: u64,
    pub archive_digest: &'a str,
    pub content_digest: &'a str,
    pub token: &'a str,
    pub runner_temp: &'a std::path::Path,
    pub deadline: std::time::Instant,
}

pub(crate) struct VerifiedPreparation {
    pub repository_id: u64,
    pub intent: velnor_actions_contract::config::CrateReleaseIntent,
    pub source_tree: String,
    pub artifact: crate::crate_release_pr_artifact::PreparationArtifact,
}

pub(crate) fn acquire_preparation<G, T>(
    request: &AcquisitionRequest<'_>,
    github: &G,
    transport: &T,
) -> Result<VerifiedPreparation, OrchestratorError>
where
    G: crate::crate_release_github::GitHubAdmissionRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
{
    let invalid = || internal("release_pr_acquisition_invalid");
    if request.artifact_id == 0
        || request.token.is_empty()
        || !request.runner_temp.is_absolute()
        || std::time::Instant::now() >= request.deadline
    {
        return Err(invalid());
    }
    let repository_id = run_evidence(request, github)?;
    let prefix = format!("repos/{}", request.binding.repository);
    let commit = github.get(&format!("{prefix}/git/commits/{}", request.binding.sha))?;
    if commit["sha"].as_str() != Some(request.binding.sha) {
        return Err(invalid());
    }
    let source_tree = commit["tree"]["sha"]
        .as_str()
        .ok_or_else(invalid)?
        .to_owned();
    let trees = crate::crate_release_pr_tree::GitHubTrees {
        api: github,
        repository: request.binding.repository,
    };
    let intent_bytes = read_reviewed_file(
        github,
        &trees,
        &source_tree,
        ".github/velnor-release-intent.toml",
    )?;
    let intent = crate::parse_crate_release_intent(&intent_bytes)?;
    if !crate::release_identity::same_repository(&intent.repository, request.binding.repository)
        || intent.default_branch != request.binding.branch
    {
        return Err(invalid());
    }
    let workflow = read_reviewed_file(
        github,
        &trees,
        &source_tree,
        ".github/workflows/release.yml",
    )?;
    verify_reviewed_workflow(&workflow, &intent)?;
    let bytes = download_preparation(request, github, transport, repository_id)?;
    let binding = crate::crate_release_pr_artifact::PreparationBinding {
        repository: request.binding.repository.to_owned(),
        workflow_sha: request.binding.sha.to_owned(),
        source_sha: request.binding.sha.to_owned(),
        run_id: request.binding.run_id,
        run_attempt: request.binding.attempt,
    };
    let allowed = intent.pr_write_paths.iter().cloned().collect();
    let artifact = crate::crate_release_pr_artifact::verify_preparation_artifact(
        &bytes,
        request.content_digest,
        &binding,
        &allowed,
    )
    .map_err(|_| invalid())?;
    Ok(VerifiedPreparation {
        repository_id,
        intent,
        source_tree,
        artifact,
    })
}

fn download_preparation<G, T>(
    request: &AcquisitionRequest<'_>,
    github: &G,
    transport: &T,
    repository_id: u64,
) -> Result<Vec<u8>, OrchestratorError>
where
    G: crate::crate_release_github::GitHubAdmissionRead,
    T: crate::crate_release_tool_download::ToolDownloadTransport,
{
    let invalid = || internal("release_pr_acquisition_invalid");
    let initial = current_preparation_artifact(request, github, repository_id)?;
    let digest = initial.digest.strip_prefix("sha256:").ok_or_else(invalid)?;
    if digest != request.archive_digest {
        return Err(invalid());
    }
    let scratch = tempfile::Builder::new()
        .prefix(".velnor-pr-artifact-")
        .tempdir_in(request.runner_temp)
        .map_err(|_| invalid())?;
    let archive = crate::crate_release_tool_download::download_tool_artifact(
        transport,
        &crate::crate_release_tool_download::ToolDownloadRequest {
            repository: request.binding.repository,
            id: request.artifact_id,
            token: request.token,
            expected_size: initial.size_in_bytes,
            expected_sha256: digest,
            destination: &scratch.path().join("preparation.zip"),
            deadline: request.deadline,
        },
    )?;
    if run_evidence(request, github)? != repository_id
        || current_preparation_artifact(request, github, repository_id)? != initial
    {
        return Err(invalid());
    }
    crate::crate_release_tool_artifact::read_pr_preparation_archive(&archive)
}

fn run_evidence<G: crate::crate_release_github::GitHubAdmissionRead>(
    request: &AcquisitionRequest<'_>,
    github: &G,
) -> Result<u64, OrchestratorError> {
    let prefix = format!("repos/{}", request.binding.repository);
    let run = request.binding.run_id;
    let attempt = request.binding.attempt;
    let repository = github.get(&prefix)?;
    let info = github.get(&format!("{prefix}/actions/runs/{run}/attempts/{attempt}"))?;
    let jobs = github.get(&format!(
        "{prefix}/actions/runs/{run}/attempts/{attempt}/jobs?per_page=100"
    ))?;
    let branch = crate::crate_release_http::encode(request.binding.branch);
    let reference = github.get(&format!("{prefix}/git/ref/heads/{branch}"))?;
    verify_run_and_producer(&request.binding, &repository, &info, &jobs, &reference)
}

fn current_preparation_artifact<G: crate::crate_release_github::GitHubAdmissionRead>(
    request: &AcquisitionRequest<'_>,
    github: &G,
    repository_id: u64,
) -> Result<crate::crate_release_tool_artifact::ToolArtifactMetadata, OrchestratorError> {
    let prefix = format!("repos/{}", request.binding.repository);
    let list = github.get(&format!(
        "{prefix}/actions/runs/{}/artifacts?per_page=100",
        request.binding.run_id
    ))?;
    let detail = github.get(&format!(
        "{prefix}/actions/artifacts/{}",
        request.artifact_id
    ))?;
    crate::crate_release_tool_artifact::validate_pr_preparation_artifact_metadata(
        &list,
        &detail,
        request.artifact_id,
        request.binding.run_id,
        request.binding.attempt,
        repository_id,
        request.binding.sha,
    )
}

pub(crate) fn read_reviewed_file<G: crate::crate_release_github::GitHubAdmissionRead + ?Sized>(
    github: &G,
    trees: &crate::crate_release_pr_tree::GitHubTrees<'_>,
    root: &str,
    path: &str,
) -> Result<Vec<u8>, OrchestratorError> {
    use crate::crate_release_pr_tree::TreeRead as _;
    use base64::Engine as _;
    let invalid = || internal("release_pr_reviewed_file_invalid");
    let mut tree = root.to_owned();
    let mut parts = path.split('/').peekable();
    while let Some(name) = parts.next() {
        let entries = trees.read_tree(&tree)?;
        let entry = entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(invalid)?;
        if parts.peek().is_some() {
            if entry.mode != "040000" {
                return Err(invalid());
            }
            tree.clone_from(&entry.sha);
        } else {
            if !matches!(entry.mode.as_str(), "100644" | "100755") {
                return Err(invalid());
            }
            let response = github.get(&format!(
                "repos/{}/git/blobs/{}",
                trees.repository, entry.sha
            ))?;
            if response["sha"].as_str() != Some(&entry.sha)
                || response["encoding"].as_str() != Some("base64")
            {
                return Err(invalid());
            }
            let content = response["content"].as_str().ok_or_else(invalid)?;
            if content.len() > 2 * 1024 * 1024 {
                return Err(invalid());
            }
            let normalized: String = content
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(normalized)
                .map_err(|_| invalid())?;
            if response["size"].as_u64() != Some(bytes.len() as u64)
                || bytes.len() > 1024 * 1024
                || crate::release_policy_admission::git_blob_sha1(&bytes)? != entry.sha
            {
                return Err(invalid());
            }
            return Ok(bytes);
        }
    }
    Err(invalid())
}

#[cfg(test)]
#[path = "crate_release_pr_acquisition_fixture.rs"]
pub(super) mod integration_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn trusts_only_current_successful_producer_run() -> Result<(), Box<dyn std::error::Error>> {
        let sha = "a".repeat(40);
        let binding = RunBinding {
            repository: "example/demo",
            branch: "main",
            sha: &sha,
            run_id: 42,
            attempt: 1,
        };
        let repository = json!({"id":7,"full_name":"example/demo","default_branch":"main"});
        let run = json!({"id":42,"run_attempt":1,"path":".github/workflows/release.yml","event":"push","status":"in_progress","head_sha":sha,"head_branch":"main","repository":{"id":7,"full_name":"example/demo"},"head_repository":{"id":7,"full_name":"example/demo"}});
        let jobs = json!({"total_count":1,"jobs":[{"id":5,"name":"Release preparation","run_id":42,"run_attempt":1,"head_sha":sha,"status":"completed","conclusion":"success"}]});
        let reference = json!({"ref":"refs/heads/main","object":{"type":"commit","sha":sha}});
        assert_eq!(
            verify_run_and_producer(&binding, &repository, &run, &jobs, &reference)?,
            7
        );
        for (target, path, value) in [
            (0, "/default_branch", json!("other")),
            (1, "/event", json!("workflow_dispatch")),
            (1, "/head_sha", json!("b".repeat(40))),
            (1, "/run_attempt", json!(2)),
            (1, "/path", json!(".github/workflows/other.yml")),
            (1, "/head_repository/id", json!(8)),
            (2, "/jobs/0/conclusion", json!("failure")),
            (2, "/jobs/0/run_id", json!(43)),
            (2, "/total_count", json!(2)),
            (3, "/object/sha", json!("b".repeat(40))),
        ] {
            let (mut a, mut b, mut c, mut d) = (
                repository.clone(),
                run.clone(),
                jobs.clone(),
                reference.clone(),
            );
            let object = match target {
                0 => &mut a,
                1 => &mut b,
                2 => &mut c,
                _ => &mut d,
            };
            *object.pointer_mut(path).ok_or("missing fixture key")? = value;
            assert!(
                verify_run_and_producer(&binding, &a, &b, &c, &d).is_err(),
                "accepted {path}"
            );
        }
        let mut duplicate = jobs.clone();
        duplicate["jobs"]
            .as_array_mut()
            .ok_or("jobs")?
            .push(jobs["jobs"][0].clone());
        duplicate["total_count"] = json!(2);
        assert!(
            verify_run_and_producer(&binding, &repository, &run, &duplicate, &reference).is_err()
        );
        Ok(())
    }
}
