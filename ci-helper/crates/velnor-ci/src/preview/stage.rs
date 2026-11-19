// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use super::attest::{Attestor, Authority, Subject};
use super::json::{Field, Res, require};
use super::net::{Net, repo_path};
use super::release::{Release, download_verified_release, verify_annotated_tag};
use super::{REPOSITORY, REPOSITORY_ID};
use crate::error::CiError;

const PUBLISH_JOB: &str = "Source preview publication / Publish source preview";

fn run_path(id: u64, attempt: u64) -> String {
    repo_path(&format!("/actions/runs/{id}/attempts/{attempt}"))
}

/// Port of `verify_run`: the workflow run a preview names ran on the source commit.
/// Returns its API path.
fn verify_run(net: &dyn Net, preview: &Value, prefix: &str, event: &str) -> Res<String> {
    let id = preview.at(&format!("{prefix}_run_id"));
    let attempt = preview.at(&format!("{prefix}_run_attempt"));
    let (Some(id), Some(attempt)) = (id.as_u64(), attempt.as_u64()) else {
        return Err(CiError::new("source preview run identity"));
    };
    let path = run_path(id, attempt);
    let run = net.api(&path)?;
    require(run.at("id").is_u(id) && run.at("run_attempt").is_u(attempt))?;
    require(run.at("event").is_s(event) && run.at("head_sha") == preview.at("source_sha"))?;
    let branch = if matches!(prefix, "source" | "draft" | "signer") {
        preview.at("tag")
    } else {
        &Value::String("main".to_owned())
    };
    require(run.at("head_branch") == branch)?;
    require(run.at("path") == preview.at(&format!("{prefix}_workflow_path")))?;
    require(run.at("status").is_s("completed"))?;
    if matches!(prefix, "ci" | "qualification") {
        require(run.at("conclusion").is_s("success"))?;
    }
    let repo = run.at("head_repository");
    require(repo.at("id").is_u(REPOSITORY_ID) && repo.at("full_name").is_s(REPOSITORY))?;
    Ok(path)
}

/// Port of `verify_required_job`: exactly one job called `name`, and it succeeded.
/// The whole job list is read, up to 1000 jobs.
fn verify_required_job(net: &dyn Net, run: (u64, u64), name: &str, source: &str) -> Res<Value> {
    let path = run_path(run.0, run.1);
    let mut jobs: Vec<Value> = Vec::new();
    let mut total: Option<u64> = None;
    for page in 1..=10 {
        let response = net.api(&format!("{path}/jobs?per_page=100&page={page}"))?;
        let count = response.at("total_count");
        let values = response.at("jobs").as_array();
        require(count.is_pos() && values.is_some())?;
        let count = count.as_u64().unwrap_or(0);
        let values = values.map_or(&[][..], Vec::as_slice);
        require(total.is_none_or(|t| t == count) && count <= 1_000)?;
        total = Some(count);
        jobs.extend(values.iter().cloned());
        require(jobs.len() as u64 <= count)?;
        if jobs.len() as u64 == count {
            break;
        }
        require(values.len() == 100)?;
    }
    require(Some(jobs.len() as u64) == total)?;
    require(jobs.iter().all(|j| j.at("id").is_pos()))?;
    let mut ids: Vec<u64> = jobs.iter().filter_map(|j| j.at("id").as_u64()).collect();
    ids.sort_unstable();
    ids.dedup();
    require(ids.len() == jobs.len())?;
    require(jobs.iter().all(|j| {
        j.at("run_id").is_u(run.0)
            && j.at("run_attempt").is_u(run.1)
            && j.at("head_sha").is_s(source)
    }))?;
    let matches: Vec<&Value> = jobs.iter().filter(|j| j.at("name").is_s(name)).collect();
    require(matches.len() == 1)?;
    let job = matches[0];
    require(job.at("status").is_s("completed") && job.at("conclusion").is_s("success"))?;
    Ok(job.clone())
}

fn pair(preview: &Value, prefix: &str) -> Res<(u64, u64)> {
    let id = preview.at(&format!("{prefix}_run_id")).as_u64();
    let attempt = preview.at(&format!("{prefix}_run_attempt")).as_u64();
    id.zip(attempt)
        .ok_or_else(|| CiError::new("source preview run identity"))
}

/// Port of `verify_publication_jobs`: the admit, draft, attest and publish jobs ran and
/// succeeded, and the publish job is the one the record pins.
fn verify_publication_jobs(net: &dyn Net, preview: &Value) -> Res<()> {
    let source = preview.at("source_sha").text();
    for (role, name) in [
        (
            "source",
            "Source preview publication / Admit source preview",
        ),
        ("draft", "Source preview publication / Draft source preview"),
        (
            "signer",
            "Source preview publication / Attest source preview",
        ),
    ] {
        verify_run(net, preview, role, "push")?;
        verify_required_job(net, pair(preview, role)?, name, source)?;
    }
    let mut publisher = preview.clone();
    publisher["signer_run_attempt"] = preview.at("publisher_run_attempt").clone();
    verify_run(net, &publisher, "signer", "push")?;
    let job = verify_required_job(net, pair(&publisher, "signer")?, PUBLISH_JOB, source)?;
    require(job.at("id") == preview.at("publisher_job_id"))?;
    let job_id = preview.at("publisher_job_id");
    let pinned = net.api(&repo_path(&format!("/actions/jobs/{job_id}")))?;
    require(pinned.at("id") == job_id && pinned.at("run_id") == preview.at("signer_run_id"))?;
    require(
        pinned.at("run_attempt") == preview.at("publisher_run_attempt")
            && pinned.at("head_sha") == preview.at("source_sha"),
    )?;
    require(pinned.at("name").is_s(PUBLISH_JOB) && pinned.at("status").is_s("completed"))?;
    require(pinned.at("conclusion").is_s("success"))
}

/// Port of `verify_target_producers`: both payloads were built and qualified by a
/// successful run on `main`, and their artifacts are the ones the record pins.
fn verify_target_producers(net: &dyn Net, preview: &Value) -> Res<()> {
    let source = preview.at("source_sha").text();
    for (target, label) in [("linux", "Linux"), ("darwin", "Darwin")] {
        let (run_id, attempt) = pair(preview, &format!("{target}_producer"))?;
        let run = net.api(&run_path(run_id, attempt))?;
        require(run.at("id").is_u(run_id) && run.at("run_attempt").is_u(attempt))?;
        require(run.at("head_sha").is_s(source) && run.at("head_branch").is_s("main"))?;
        require(run.at("event").is_s("push"))?;
        require(run.at("path").text().split('@').next() == Some(super::SIGNER_WORKFLOW))?;
        require(run.at("status").is_s("completed") && run.at("conclusion").is_s("success"))?;
        require(run.at("head_repository").at("id").is_u(REPOSITORY_ID))?;
        verify_required_job(
            net,
            (run_id, attempt),
            &format!("Build source preview {label}"),
            source,
        )?;
        let job = verify_required_job(
            net,
            (run_id, attempt),
            &format!("Qualify source preview {label}"),
            source,
        )?;
        require(job.at("id") == preview.at(&format!("{target}_qualification_job_id")))?;
        let artifact_id = preview.at(&format!("{target}_producer_payload_artifact_id"));
        let artifact = net.api(&repo_path(&format!("/actions/artifacts/{artifact_id}")))?;
        require(artifact.at("id") == artifact_id && artifact.at("expired") == &Value::Bool(false))?;
        require(
            artifact
                .at("name")
                .is_s(&format!("source-preview-payload-{target}-{attempt}")),
        )?;
        let digest = format!(
            "sha256:{}",
            preview
                .at(&format!("{target}_producer_payload_archive_sha256"))
                .text()
        );
        require(artifact.at("digest").is_s(&digest))?;
        let workflow = artifact.at("workflow_run");
        require(workflow.at("id").is_u(run_id) && workflow.at("head_sha").is_s(source))?;
    }
    Ok(())
}

/// Port of `verify_preview`: everything that must hold before a preview is trusted.
/// Returns the release assets, which the caller installs.
pub fn verify_preview(
    net: &dyn Net,
    attestor: &dyn Attestor,
    preview: &Value,
    authority: &Authority,
) -> Res<Release> {
    let source = preview.at("source_sha").text();
    let ci = verify_run(net, preview, "ci", "push")?;
    let qualification = verify_run(net, preview, "qualification", "workflow_dispatch")?;
    let (ci_run, qual_run) = (pair(preview, "ci")?, pair(preview, "qualification")?);
    require(
        ci == run_path(ci_run.0, ci_run.1) && qualification == run_path(qual_run.0, qual_run.1),
    )?;
    verify_required_job(net, ci_run, "Required", source)?;
    verify_required_job(net, qual_run, "Source qualification", source)?;
    verify_target_producers(net, preview)?;
    verify_publication_jobs(net, preview)?;
    let tag_sha = verify_annotated_tag(net, preview)?;
    let release = download_verified_release(net, preview)?;
    let compare = net.api(&repo_path(&format!(
        "/compare/{}...{source}",
        authority.sha
    )))?;
    require(compare.at("status").is_s("ahead"))?;
    require(
        compare
            .at("merge_base_commit")
            .at("sha")
            .is_s(&authority.sha),
    )?;
    let subject = Subject::from_preview(preview)?;
    attestor.verify(
        &release.receipt,
        &release.bundle,
        &subject,
        authority,
        net.deadline(),
    )?;
    require(verify_annotated_tag(net, preview)? == tag_sha)?;
    Ok(release)
}
