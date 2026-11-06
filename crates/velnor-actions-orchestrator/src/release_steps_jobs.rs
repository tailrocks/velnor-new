// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{JobParts, Step, OrchestratorError, controller_step, conditional, ReleaseJobSpec, receipt_step, action_step, UPLOAD_ARTIFACT_USES, BTreeMap, RUST_ARTIFACT, RUST_ARCHIVE, StepId, StepRole, ReleaseRole, JobTimeout, JobPermissions, upload_record_step};

fn common_source_steps(
    parts: &JobParts<'_>,
    require: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = vec![
        parts.acquire.clone(),
        parts.policy.clone(),
        controller_step(
            parts,
            if require {
                "source-require"
            } else {
                "source-admit"
            },
        )?,
        conditional(parts.source.clone()),
    ];
    if require {
        steps.push(controller_step(parts, "toolchain-acquire")?);
    } else {
        steps.push(parts.setup.clone());
        steps.push(parts.prepare_rust.clone());
    }
    steps.push(parts.verify_rust.clone());
    Ok(steps)
}

pub(super) fn preflight_job(parts: &JobParts<'_>) -> Result<ReleaseJobSpec, OrchestratorError> {
    let mut steps = common_source_steps(parts, false)?;
    steps.push(conditional(parts.semver.clone()));
    steps.push(controller_step(parts, "candidate-receipt")?);
    steps.push(receipt_step()?);
    steps.push(conditional(
        velnor_actions_workflow_renderer::release_gates::release_rust_archive_step(
            parts.rust_version,
            parts.label,
        )?,
    ));
    let mut upload = action_step(
        "Upload release Rust toolchain",
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), RUST_ARTIFACT.to_owned()),
            ("path".to_owned(), RUST_ARCHIVE.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "7".to_owned()),
        ]),
    )?;
    upload.id = Some(StepId::ReleaseRustUpload);
    upload.role = Some(StepRole::ReleaseRustUpload);
    steps.push(conditional(upload));
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Preflight,
        display_name: "Release preflight".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: Vec::new(),
        outputs: BTreeMap::from([
            ("rust_artifact_id".to_owned(), "${{ steps.release-rust-upload.outputs.artifact-id }}".to_owned()),
            ("receipt_artifact_id".to_owned(), "${{ steps.release-receipt-upload.outputs.artifact-id }}".to_owned()),
            (
                "eligible".to_owned(),
                "${{ steps.release-admission.outputs.eligible }}".to_owned(),
            ),
            (
                "source_sha".to_owned(),
                "${{ steps.release-admission.outputs.source_sha }}".to_owned(),
            ),
        ]),
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Preflight),
        steps,
    })
}

fn publish_steps(
    parts: &JobParts<'_>,
    bootstrap: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    Ok(vec![
        velnor_actions_workflow_renderer::release_budget::capture_step()?,
        velnor_actions_workflow_renderer::release_root_publisher::publisher_step(
            bootstrap,
            parts.generator_version,
            parts.generator_source_sha,
            parts.generator_manifest_sha256,
        )?,
    ])
}

pub(super) fn publish_job(
    parts: &JobParts<'_>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::PublishOidc,
        display_name: "Release publish".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: vec!["release-preflight".to_owned()],
        outputs: BTreeMap::new(),
        condition: Some(parts.gate.to_owned()),
        environment: Some(parts.environment.to_owned()),
        permissions: JobPermissions::expected(ReleaseRole::PublishOidc),
        steps: publish_steps(parts, false)?,
    })
}

pub(super) fn bootstrap_job(
    parts: &JobParts<'_>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::PublishBootstrap,
        display_name: "Release bootstrap publish".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: vec!["release-preflight".to_owned()],
        outputs: BTreeMap::new(),
        condition: Some(parts.gate.to_owned()),
        environment: Some(parts.environment.to_owned()),
        permissions: JobPermissions::expected(ReleaseRole::PublishBootstrap),
        steps: publish_steps(parts, true)?,
    })
}

pub(super) fn reconcile_job(
    parts: &JobParts<'_>,
    bootstrap: bool,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    let publisher = if bootstrap {
        "release-publish-bootstrap"
    } else {
        "release-publish"
    };
    let mut steps = common_source_steps(parts, true)?;
    steps.push(controller_step(parts, "reconciliation-receipt-acquire")?);
    steps.push(controller_step(parts, "reconcile")?);
    steps.push(upload_record_step(
        "Upload reconciliation record",
        "velnor-crate-release-reconciled",
        "${{ runner.temp }}/velnor-crate-release-reconciled.json",
    )?);
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Reconcile,
        display_name: "Release reconcile".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: vec!["release-preflight".to_owned(), publisher.to_owned()],
        outputs: BTreeMap::new(),
        condition: Some(format!("always() && needs.{publisher}.result != 'skipped'")),
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Reconcile),
        steps,
    })
}

