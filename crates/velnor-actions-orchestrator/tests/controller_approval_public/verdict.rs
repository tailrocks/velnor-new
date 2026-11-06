// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, io};

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::{BASE_APPROVALS, BASE_LOCK, fixture, lock_for_version, rotated_approvals};

fn active_identity(approvals: &str) -> Result<(String, String, String, String), Box<dyn Error>> {
    let approvals: serde_json::Value = serde_json::from_str(approvals)?;
    let active = &approvals["active"];
    let version = active["version"]
        .as_str()
        .ok_or_else(|| io::Error::other("active version is missing"))?;
    let commit = active["commit"]
        .as_str()
        .ok_or_else(|| io::Error::other("active commit is missing"))?;
    let linux = active["binaries"]
        .as_array()
        .and_then(|binaries| {
            binaries
                .iter()
                .find(|binary| binary["target"] == "x86_64-unknown-linux-gnu")
        })
        .ok_or_else(|| io::Error::other("active linux binary is missing"))?;
    let artifact = linux["artifact"]
        .as_str()
        .ok_or_else(|| io::Error::other("active artifact is missing"))?;
    let sha256 = linux["sha256"]
        .as_str()
        .ok_or_else(|| io::Error::other("active sha256 is missing"))?;
    Ok((
        version.to_owned(),
        commit.to_owned(),
        artifact.to_owned(),
        sha256.to_owned(),
    ))
}

fn rendered_workflow(approvals: &str, lock: &str) -> Result<serde_json::Value, Box<dyn Error>> {
    let temp = fixture(Some(approvals), lock)?;
    let tree = render_staged_tree(&prepare(temp.path())?)?;
    let workflow = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| io::Error::other("ci workflow is not rendered"))?;
    Ok(serde_saphyr::from_str(workflow)?)
}

fn acquire_environment(
    workflow: &serde_json::Value,
    job_id: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, Box<dyn Error>> {
    let jobs = workflow
        .get("jobs")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| io::Error::other("workflow jobs are missing"))?;
    let job = jobs
        .get(job_id)
        .ok_or_else(|| io::Error::other(format!("{job_id} job is not rendered")))?;
    let steps = job
        .get("steps")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| io::Error::other(format!("{job_id} steps are missing")))?;
    let mut acquire_steps = steps.iter().filter(|step| {
        step.get("name").and_then(serde_json::Value::as_str) == Some("Acquire Velnor")
    });
    let acquire = acquire_steps
        .next()
        .ok_or_else(|| io::Error::other(format!("{job_id} acquire step is not rendered")))?;
    if acquire_steps.next().is_some() {
        return Err(io::Error::other(format!("{job_id} has multiple acquire steps")).into());
    }

    let mut environment = workflow
        .get("env")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(job_environment) = job.get("env").and_then(serde_json::Value::as_object) {
        environment.extend(job_environment.clone());
    }
    if let Some(step_environment) = acquire.get("env").and_then(serde_json::Value::as_object) {
        environment.extend(step_environment.clone());
    }
    Ok(environment)
}

fn assert_acquire_matches(
    environment: &serde_json::Map<String, serde_json::Value>,
    commit: &str,
    artifact: &str,
    sha256: &str,
    label: &str,
) {
    assert_eq!(
        environment.get("VELNOR_RELEASE_COMMIT"),
        Some(&serde_json::Value::String(commit.to_owned())),
        "{label} Acquire Velnor must bind the release commit"
    );
    assert_eq!(
        environment.get("VELNOR_ASSET_URL"),
        Some(&serde_json::Value::String(artifact.to_owned())),
        "{label} Acquire Velnor must bind the artifact URL"
    );
    assert_eq!(
        environment.get("VELNOR_ASSET_SHA256"),
        Some(&serde_json::Value::String(sha256.to_owned())),
        "{label} Acquire Velnor must bind the artifact digest"
    );
}

fn published_identity() -> Result<(String, String, String), Box<dyn Error>> {
    let manifest = velnor_actions_mise::catalog::lock::published_verdict_manifest();
    let linux = manifest
        .record_for_target("x86_64-unknown-linux-gnu")
        .ok_or_else(|| io::Error::other("published Linux binary is missing"))?;
    Ok((
        manifest.commit.clone(),
        linux.artifact.clone(),
        linux.sha256.clone(),
    ))
}

#[test]
fn producer_acquires_bind_published_and_active_release_identities() -> Result<(), Box<dyn Error>> {
    let (published_commit, published_artifact, published_sha256) = published_identity()?;
    let (baseline_version, baseline_commit, baseline_artifact, baseline_sha256) =
        active_identity(BASE_APPROVALS)?;
    assert_eq!(baseline_version, "0.1.74");
    assert_eq!(baseline_commit, published_commit);
    assert_eq!(baseline_artifact, published_artifact);
    assert_eq!(baseline_sha256, published_sha256);

    let baseline = rendered_workflow(BASE_APPROVALS, BASE_LOCK)?;
    assert_acquire_matches(
        &acquire_environment(&baseline, "required")?,
        &published_commit,
        &published_artifact,
        &published_sha256,
        "Required",
    );

    let rotated = rotated_approvals();
    let rotated_lock = lock_for_version("0.1.75", "dddddddddddddddddddddddddddddddddddddddd");
    let (rotated_version, rotated_commit, rotated_artifact, rotated_sha256) =
        active_identity(&rotated)?;
    assert_eq!(rotated_version, "0.1.75");
    assert_ne!(rotated_commit, published_commit);
    assert_ne!(rotated_artifact, published_artifact);
    assert_ne!(rotated_sha256, published_sha256);

    let rotated_workflow = rendered_workflow(&rotated, &rotated_lock)?;
    assert_acquire_matches(
        &acquire_environment(&rotated_workflow, "required")?,
        &published_commit,
        &published_artifact,
        &published_sha256,
        "Required",
    );
    assert_acquire_matches(
        &acquire_environment(&rotated_workflow, "plan")?,
        &rotated_commit,
        &rotated_artifact,
        &rotated_sha256,
        "Plan",
    );
    Ok(())
}
