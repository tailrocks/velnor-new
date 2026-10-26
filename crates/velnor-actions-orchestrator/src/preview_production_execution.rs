// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    aggregate::AdmittedLeaves,
    handoff::{self, AssetProof},
    leaf_manifest::{self, PreviewAggregateManifest, PreviewLeafManifest},
    proof::{ProofGuard, publish_proof},
};
use crate::{
    OrchestratorError,
    internal::internal,
    package_release_artifacts::{
        PackageOutputIdentity, preview_payload::verify_payload_output, verify_package_output,
    },
    preview_producer::{PreviewTaskSpec, execute_preview_task},
    retrieve_reports::{MAX_RETRIEVE_PLAN_BYTES, staged_reads::read_staged_bytes_until},
};
use std::path::Path;
use velnor_actions_contract::{
    config::RustBinaryReleaseConfig,
    preview_production::{ReleaseProductionRequest, ReleaseProductionStage},
};
use velnor_actions_mise::CheckDeadline;

struct Publication<'a> {
    output: &'a Path,
    manifest: &'a str,
    temp: &'a Path,
    raw: &'a Path,
    request: &'a ReleaseProductionRequest,
    deadline: CheckDeadline,
}

pub(super) fn execute(
    root: &Path,
    qualified_tools: &[velnor_actions_contract::config::QualifiedTool],
    temp: &Path,
    preview: &RustBinaryReleaseConfig,
    request: &ReleaseProductionRequest,
    deadline: CheckDeadline,
    guard: &mut ProofGuard,
) -> Result<(), OrchestratorError> {
    let progress = crate::preview_producer::progress::Progress::artifacts();
    let admitted = match request.stage {
        ReleaseProductionStage::Payload { .. } => None,
        ReleaseProductionStage::Assemble { .. } => {
            Some(super::acquisition::load(temp, request, preview, deadline)?)
        }
    };
    let execution_request = admitted
        .as_ref()
        .map_or_else(|| request.clone(), |leaves| leaves.request.clone());
    let task = handoff::write_task_request(
        temp,
        &execution_request,
        admitted
            .as_ref()
            .map_or(&[], |leaves| leaves.inputs.as_slice()),
        deadline,
    )?;
    let identity = crate::release_task_identity::ReleaseTaskIdentity::derive(
        &execution_request.plan,
        &execution_request.context.source_sha,
        execution_request.context.commit_count,
    )?;
    let producer = execute_preview_task(
        root,
        temp,
        PreviewTaskSpec {
            identity: &identity,
            source_ref: &execution_request.context.source_ref,
            preview,
            handoff: &task.path,
        },
        qualified_tools,
        deadline,
    )?;
    task.verify(deadline)?;
    let output = root.join(&preview.producer.output_directory);
    let manifest = preview
        .producer
        .manifest
        .strip_prefix(&format!("{}/", preview.producer.output_directory))
        .ok_or_else(|| internal("preview_manifest_not_under_output"))?;
    let raw = fresh_raw_output(temp, deadline)?;
    let operation = Publication {
        output: &output,
        manifest,
        temp,
        raw: &raw,
        request: &execution_request,
        deadline,
    };
    progress.mark(crate::preview_producer::progress::Phase::Artifacts);
    match admitted {
        None => publish_leaf(&operation, producer, guard),
        Some(leaves) => publish_aggregate(&operation, producer, preview, &leaves, guard),
    }?;
    progress.mark(crate::preview_producer::progress::Phase::ArtifactsComplete);
    Ok(())
}

fn publish_leaf(
    operation: &Publication<'_>,
    producer: crate::preview_producer::PreviewProducerProof,
    guard: &mut ProofGuard,
) -> Result<(), OrchestratorError> {
    let Publication {
        output,
        manifest,
        temp,
        raw,
        request,
        deadline,
    } = *operation;
    let verified = verify_payload_output(
        output,
        manifest,
        &temp.join("preview-verified"),
        request,
        deadline,
    )?;
    let mut assets = Vec::new();
    for artifact in [verified.archive, verified.sbom] {
        assets.push(handoff::stage_asset(&artifact, raw, deadline)?);
    }
    assets.sort_by(|a, b| a.name.cmp(&b.name));
    let proof = PreviewLeafManifest {
        schema: leaf_manifest::LEAF_SCHEMA.to_owned(),
        request: request.clone(),
        producer,
        assets,
    };
    let bytes = leaf_manifest::encode(&proof)?;
    PreviewLeafManifest::parse(&bytes, request)?;
    publish_proof(&raw.join(handoff::LEAF_MANIFEST), &bytes, deadline, guard)
}

fn publish_aggregate(
    operation: &Publication<'_>,
    producer: crate::preview_producer::PreviewProducerProof,
    preview: &RustBinaryReleaseConfig,
    leaves: &AdmittedLeaves,
    guard: &mut ProofGuard,
) -> Result<(), OrchestratorError> {
    let Publication {
        output,
        manifest,
        temp,
        raw,
        request,
        deadline,
    } = *operation;
    leaves.recheck(deadline)?;
    let manifest_bytes = read_manifest(output, manifest, deadline)?;
    let verified = verify_package_output(
        output,
        manifest,
        &temp.join("preview-verified"),
        &request.plan.products,
        &preview.companions,
        PackageOutputIdentity {
            source_commit: &request.context.source_sha,
            source_ref: &request.context.source_ref,
            version: &request.context.version,
        },
    )?;
    if read_manifest(output, manifest, deadline)? != manifest_bytes {
        return Err(internal("preview_source_manifest_changed"));
    }
    let mut assets = Vec::<AssetProof>::new();
    for artifact in verified.artifacts {
        assets.push(handoff::stage_asset(&artifact, raw, deadline)?);
    }
    assets.sort_by(|a, b| a.name.cmp(&b.name));
    leaves.require_unchanged_output(&assets)?;
    leaves.recheck(deadline)?;
    handoff::write_upload(&raw.join("package-output.json"), &manifest_bytes, deadline)?;
    let proof = PreviewAggregateManifest {
        schema: leaf_manifest::AGGREGATE_SCHEMA.to_owned(),
        request: request.clone(),
        producer,
        leaves: leaves.manifests.clone(),
        assets,
    };
    proof.validate_source(preview)?;
    let bytes = leaf_manifest::encode(&proof)?;
    PreviewAggregateManifest::parse(
        &bytes,
        &request.policy_sha256,
        &request.plan,
        &request.context,
    )?;
    publish_proof(
        &raw.join(handoff::AGGREGATE_MANIFEST),
        &bytes,
        deadline,
        guard,
    )
}

fn read_manifest(
    output: &Path,
    manifest: &str,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    read_staged_bytes_until(&output.join(manifest), MAX_RETRIEVE_PLAN_BYTES, || {
        deadline
            .remaining()
            .map(|_| ())
            .map_err(|_| "preview_deadline")
    })
    .map_err(|_| internal("preview_manifest_read"))
}

pub(super) fn fresh_raw_output(
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<std::path::PathBuf, OrchestratorError> {
    handoff::checkpoint(deadline)?;
    handoff::fresh_directory(temp, "release-output")
}
