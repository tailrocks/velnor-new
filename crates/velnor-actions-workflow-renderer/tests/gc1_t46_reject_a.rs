// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_rejects_identity(
    fixture: &Fixture,
    identity: &str,
) -> Result<(), Box<dyn Error>> {
    expect_rejection(
        fixture,
        "identity",
        &replace_once(identity, "tailrocks/velnor-new", "other/repo")?,
        "identity-repository",
        "IDENTITY-REJECTED",
        Some("Publication repository is incorrect."),
    )?;
    expect_rejection(
        fixture,
        "identity",
        &replace_once(identity, "\"workflow_dispatch\"", "\"push\"")?,
        "identity-event",
        "IDENTITY-REJECTED",
        Some("Publication is only for workflow_dispatch on refs/heads/main."),
    )?;
    expect_rejection(
        fixture,
        "identity",
        &replace_once(
            identity,
            "\"ref\":\"refs/heads/main\"",
            "\"ref\":\"refs/heads/other\"",
        )?,
        "identity-ref",
        "IDENTITY-REJECTED",
        Some("Publication is only for workflow_dispatch on refs/heads/main."),
    )?;
    check_rejects_identity_fields(fixture, identity)?;
    Ok(())
}

pub(crate) fn check_rejects_identity_fields(
    fixture: &Fixture,
    identity: &str,
) -> Result<(), Box<dyn Error>> {
    expect_rejection(
        fixture,
        "identity",
        &replace_once(
            identity,
            &format!("\"authority_commit\":\"{SOURCE_SHA}\""),
            "\"authority_commit\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"",
        )?,
        "identity-authority",
        "IDENTITY-REJECTED",
        Some("Workflow authority SHA is different from source SHA."),
    )?;
    expect_rejection(
        fixture,
        "identity",
        &replace_once(
            identity,
            "\"repository_id\":1390620900",
            "\"repository_id\":\"x\"",
        )?,
        "identity-repository-id",
        "IDENTITY-REJECTED",
        Some("GitHub repository ID is incorrect."),
    )?;
    expect_rejection(
        fixture,
        "identity",
        &replace_once(identity, "\"run_id\":7", "\"run_id\":0")?,
        "identity-run-id",
        "IDENTITY-REJECTED",
        Some("Publication run ID is incorrect."),
    )?;
    expect_rejection(
        fixture,
        "identity",
        &replace_once(identity, ",\"artifact_id\":9", "")?,
        "identity-missing",
        "IDENTITY-REJECTED",
        Some("Publication identity has unknown or missing fields."),
    )?;
    let dup_path = fixture.root.path().join("dup.json");
    std::fs::write(&dup_path, "{\"a\":1,\"a\":2}")?;
    let dup_envelope = format!(
        "{{\"path\":{}}}",
        json_string(
            dup_path
                .to_str()
                .ok_or_else(|| fail("duplicate path left its string shape"))?
        )
    );
    expect_rejection(
        fixture,
        "identity_file",
        &dup_envelope,
        "identity-duplicate",
        "IDENTITY-REJECTED",
        Some("Duplicate JSON key: a"),
    )?;
    let oversized = vec![b' '; 2 * 1024 * 1024 + 1];
    let path = fixture.root.path().join("oversized.json");
    std::fs::write(&path, &oversized)?;
    let envelope = format!(
        "{{\"path\":{}}}",
        json_string(
            path.to_str()
                .ok_or_else(|| fail("oversized path left its string shape"))?
        )
    );
    expect_rejection(
        fixture,
        "identity_file",
        &envelope,
        "identity-oversized",
        "IDENTITY-REJECTED",
        Some("exceeds its size limit."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_authorization(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    expect_rejection(
        fixture,
        "repository",
        &replace_once(&repository_envelope(), "\"id\":1390620900", "\"id\":1")?,
        "repository-id",
        "REPOSITORY-REJECTED",
        Some("GitHub repository ID is not the same as the canonical repository ID."),
    )?;
    expect_rejection(
        fixture,
        "repository",
        &replace_once(
            &repository_envelope(),
            &format!("\"sha\":\"{SOURCE_SHA}\""),
            "\"sha\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"",
        )?,
        "repository-tip",
        "REPOSITORY-REJECTED",
        Some("Source SHA is not the current main commit."),
    )?;
    expect_rejection(
        fixture,
        "candidate_identity",
        &replace_once(
            &candidate_identity_envelope(),
            &format!("\"commit\":\"{SOURCE_SHA}\""),
            "\"commit\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"",
        )?,
        "candidate-identity-commit",
        "CANDIDATE-IDENTITY-REJECTED",
        Some("Candidate source is not the current main source."),
    )?;
    expect_rejection(
        fixture,
        "candidate_identity",
        &replace_once(
            &candidate_identity_envelope(),
            "\"run_id\":7",
            "\"run_id\":8",
        )?,
        "candidate-identity-run",
        "CANDIDATE-IDENTITY-REJECTED",
        Some("Candidate run identity is different from the publication run identity."),
    )?;
    expect_rejection(
        fixture,
        "artifact",
        &replace_once(
            &artifact_envelope(),
            "\"expired\":false",
            "\"expired\":true",
        )?,
        "artifact-expired",
        "ARTIFACT-REJECTED",
        Some("Candidate artifact ID, expiry, or workflow run is incorrect."),
    )?;
    expect_rejection(
        fixture,
        "artifact",
        &format!(
            "{{\"response\":{{\"artifacts\":[]}},\"identity\":{}}}",
            identity_envelope()
        ),
        "artifact-shape",
        "ARTIFACT-REJECTED",
        Some("This workflow run must have one candidate artifact."),
    )?;
    expect_rejection(
        fixture,
        "artifact",
        &replace_once(
            &artifact_envelope(),
            "\"head_branch\":\"main\"",
            "\"head_branch\":\"other\"",
        )?,
        "artifact-branch",
        "ARTIFACT-REJECTED",
        Some("Candidate artifact ID, expiry, or workflow run is incorrect."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_ci_and_attestation(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    expect_rejection(
        fixture,
        "ci",
        &format!("{{\"listing\":{{\"workflow_runs\":[]}},\"source\":\"{SOURCE_SHA}\"}}"),
        "ci-empty",
        "CI-REJECTED",
        Some("No canonical main CI push run exists for this source."),
    )?;
    expect_rejection(
        fixture,
        "ci",
        &format!(
            "{{\"listing\":{{\"workflow_runs\":[{run}]}},\"source\":\"{SOURCE_SHA}\"}}",
            run = ci_run(11, 5, 1, "completed", "failure"),
        ),
        "ci-conclusion",
        "CI-REJECTED",
        Some("The latest canonical main CI run has no `success` conclusion."),
    )?;
    expect_rejection(
        fixture,
        "ci",
        &format!(
            "{{\"listing\":{{\"workflow_runs\":[{a},{b}]}},\"source\":\"{SOURCE_SHA}\"}}",
            a = ci_run(11, 5, 1, "completed", "success"),
            b = ci_run(12, 5, 1, "completed", "success"),
        ),
        "ci-duplicate",
        "CI-REJECTED",
        Some("The latest canonical main CI run has no `success` conclusion."),
    )?;
    expect_rejection(
        fixture,
        "attestation",
        &format!("{{\"output\":[],\"source\":\"{SOURCE_SHA}\"}}"),
        "attestation-empty",
        "ATTESTATION-REJECTED",
        Some("Attestation verification output has no records."),
    )?;
    expect_rejection(
        fixture,
        "attestation",
        &replace_once(
            &attestation_envelope(),
            "\"sourceRepositoryIdentifier\":1390620900",
            "\"sourceRepositoryIdentifier\":1",
        )?,
        "attestation-repository",
        "ATTESTATION-REJECTED",
        Some("Attestation certificate repository ID is incorrect."),
    )?;
    expect_rejection(
        fixture,
        "attestation",
        &replace_once(
            &attestation_envelope(),
            "\"githubWorkflowTrigger\":\"workflow_dispatch\"",
            "\"githubWorkflowTrigger\":\"push\"",
        )?,
        "attestation-trigger",
        "ATTESTATION-REJECTED",
        Some("Attestation certificate source or workflow identity is incorrect."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_source_and_lock(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    expect_rejection(
        fixture,
        "source_inputs",
        &replace_once(
            &source_inputs_envelope(),
            &format!("\"commit\":\"{SOURCE_SHA}\""),
            "\"commit\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"",
        )?,
        "source-inputs-commit",
        "SOURCE-INPUTS-REJECTED",
        Some("Candidate source is not the authenticated source."),
    )?;
    let tampered = source_inputs_envelope().replacen(&"b".repeat(64), &"c".repeat(64), 1);
    expect_rejection(
        fixture,
        "source_inputs",
        &tampered,
        "source-inputs-bytes",
        "SOURCE-INPUTS-REJECTED",
        Some("Build input bytes changed after candidate creation."),
    )?;
    expect_rejection(
        fixture,
        "build_lock",
        "{\"builder\":\"github.com/docker/buildx v0.25.0\",\"buildx_version\":\"0.24.0\"}",
        "build-lock-version",
        "BUILD-LOCK-REJECTED",
        Some("Candidate Buildx identity is not the same as the value in .velnor/runner.lock."),
    )?;
    expect_rejection(
        fixture,
        "build_lock",
        "{\"builder\":\"docker v0.25.0\",\"buildx_version\":\"0.25.0\"}",
        "build-lock-builder",
        "BUILD-LOCK-REJECTED",
        Some("Candidate Buildx identity is not the same as the value in .velnor/runner.lock."),
    )?;
    Ok(())
}
