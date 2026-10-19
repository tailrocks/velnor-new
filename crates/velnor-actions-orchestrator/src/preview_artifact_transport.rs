//! Current-attempt, ID-addressed acquisition of unsigned native preview leaves.
//!
//! The artifact API has no attempt field. Literal current-attempt upload outputs,
//! canonical pair/run/attempt names and reauthenticated run identity jointly bind
//! these bytes; timestamps never serve as attempt evidence. ZIP admission follows
//! in a separate owner, and these bytes alone are not an admitted payload.

use super::artifact_zip::{ZipProfile, leaf_union_compressed_bound};
use super::github::PreviewArtifactApi;
use std::{fs::File, path::Path};
#[path = "preview_artifact_transport_file.rs"]
mod archive_file;
use crate::{OrchestratorError, internal::internal};
use archive_file::SealedArchive;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::PackageReleasePlan;
use velnor_actions_contract::preview_production::{
    PREVIEW_ASSEMBLE_JOB_ID, PayloadArtifactRef, PreviewProductionContext,
    PreviewProductionRequest, PreviewProductionStage, preview_payload_artifact_name,
};
use velnor_actions_mise::CheckDeadline;

#[path = "preview_artifact_transport_aggregate.rs"]
mod aggregate;
pub(crate) use aggregate::acquire_preview_aggregate;

const MAX_ARTIFACTS: usize = 1000;

/// Authenticated service archive bytes, still requiring closed ZIP/leaf admission.
pub(crate) struct AdmittedPreviewArtifact {
    pub reference: PayloadArtifactRef,
    pub archive: SealedArchive,
}

/// Acquire exactly the literal assembly references under one cumulative budget.
/// The caller must first admit the request against its trusted source plan and
/// authenticated job context; this function independently reauthenticates GitHub.
pub(crate) fn acquire_preview_artifacts(
    request: &PreviewProductionRequest,
    directory: &Path,
    deadline: CheckDeadline,
    mut consume: impl FnMut(AdmittedPreviewArtifact) -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    require_consumer_environment(PreviewConsumer::Assembler)?;
    let api = PreviewArtifactApi::new(&request.plan.repository);
    deadline
        .scope(|| acquire(&api, request, directory, deadline, &mut consume))
        .map_err(|_| internal("preview_artifact_deadline"))?
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreviewConsumer {
    Assembler,
    Signer,
}
impl PreviewConsumer {
    pub(in crate::package_release_publish) fn job_name(self) -> &'static str {
        match self {
            Self::Assembler => PREVIEW_ASSEMBLE_JOB_ID,
            Self::Signer => {
                velnor_actions_contract::package_release::PackageReleaseChannel::Preview
                    .signer_job_id()
            }
        }
    }
}
fn require_consumer_environment(consumer: PreviewConsumer) -> Result<(), OrchestratorError> {
    validate_consumer_environment(consumer, std::env::var("GITHUB_JOB").ok().as_deref())?;
    // This new read capability must not fall back to ambient cached gh login.
    let token_present = std::env::var("GH_TOKEN").is_ok_and(|token| !token.trim().is_empty());
    if !token_present {
        return Err(internal("preview_artifact_read_token_missing"));
    }
    Ok(())
}
fn validate_consumer_environment(
    consumer: PreviewConsumer,
    actual: Option<&str>,
) -> Result<(), OrchestratorError> {
    if actual != Some(consumer.job_name()) {
        return Err(internal("preview_artifact_consumer_environment"));
    }
    Ok(())
}

pub(super) trait ArtifactReadApi {
    fn authenticate(
        &self,
        plan: &PackageReleasePlan,
        context: &PreviewProductionContext,
        consumer: PreviewConsumer,
    ) -> Result<(), OrchestratorError>;
    fn page(&self, run: u64, page: usize) -> Result<ArtifactPage, OrchestratorError>;
    fn download(
        &self,
        id: u64,
        size: u64,
        output: &mut File,
        deadline: CheckDeadline,
    ) -> Result<(), OrchestratorError>;
}

fn acquire(
    api: &impl ArtifactReadApi,
    request: &PreviewProductionRequest,
    directory: &Path,
    deadline: CheckDeadline,
    consume: &mut impl FnMut(AdmittedPreviewArtifact) -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    request
        .validate()
        .map_err(|_| internal("preview_artifact_request"))?;
    let PreviewProductionStage::Assemble { inputs } = &request.stage else {
        return Err(internal("preview_artifact_assembly_required"));
    };
    checkpoint(deadline)?;
    api.authenticate(&request.plan, &request.context, PreviewConsumer::Assembler)?;
    let inventory = inventory(api, request.context.run_id, deadline)?;
    let mut total = 0_u64;
    for reference in inputs {
        checkpoint(deadline)?;
        let artifact = inventory
            .get(&reference.artifact_id)
            .ok_or_else(|| internal("preview_artifact_id_missing"))?;
        validate_selected(artifact, reference, request)?;
        total = total
            .checked_add(artifact.size_in_bytes)
            .filter(|size| *size <= leaf_union_compressed_bound(inputs.len()))
            .ok_or_else(|| internal("preview_artifact_total_size"))?;
    }
    for reference in inputs {
        checkpoint(deadline)?;
        let artifact = &inventory[&reference.artifact_id];
        let mut archive = SealedArchive::create(
            directory,
            reference.artifact_id,
            artifact.size_in_bytes,
            &reference.sha256,
        )?;
        api.download(
            reference.artifact_id,
            artifact.size_in_bytes,
            archive.output(),
            deadline,
        )?;
        archive.seal(deadline)?;
        consume(AdmittedPreviewArtifact {
            reference: reference.clone(),
            archive,
        })?;
        checkpoint(deadline)?;
    }
    api.authenticate(&request.plan, &request.context, PreviewConsumer::Assembler)?;
    checkpoint(deadline)?;
    Ok(())
}

fn inventory(
    api: &impl ArtifactReadApi,
    run: u64,
    deadline: CheckDeadline,
) -> Result<BTreeMap<u64, ArtifactRecord>, OrchestratorError> {
    let mut expected = None;
    let mut records = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for page in 1..=10 {
        checkpoint(deadline)?;
        let response = api.page(run, page)?;
        if response.total_count > MAX_ARTIFACTS
            || expected.is_some_and(|count| count != response.total_count)
        {
            return Err(internal("preview_artifact_inventory_count"));
        }
        expected = Some(response.total_count);
        let remaining = response
            .total_count
            .checked_sub(records.len())
            .ok_or_else(|| internal("preview_artifact_inventory_count"))?;
        if response.artifacts.len() != remaining.min(100) {
            return Err(internal("preview_artifact_inventory_truncated"));
        }
        for artifact in response.artifacts {
            checkpoint(deadline)?;
            if artifact.id == 0 || !ids.insert(artifact.id) {
                return Err(internal("preview_artifact_inventory_duplicate"));
            }
            records.insert(artifact.id, artifact);
        }
        if records.len() == response.total_count {
            return Ok(records);
        }
    }
    Err(internal("preview_artifact_inventory_pages"))
}

fn validate_selected(
    artifact: &ArtifactRecord,
    reference: &PayloadArtifactRef,
    request: &PreviewProductionRequest,
) -> Result<(), OrchestratorError> {
    let expected_name =
        preview_payload_artifact_name(&request.plan, &reference.pair, &request.context)
            .map_err(|_| internal("preview_artifact_name"))?;
    if artifact.size_in_bytes > ZipProfile::PreviewLeaf.compressed() {
        return Err(internal("preview_artifact_metadata_identity"));
    }
    validate_metadata(
        artifact,
        reference.artifact_id,
        &reference.sha256,
        &expected_name,
        &request.plan,
        &request.context,
    )
}

fn validate_metadata(
    artifact: &ArtifactRecord,
    artifact_id: u64,
    digest: &str,
    expected_name: &str,
    plan: &PackageReleasePlan,
    context: &PreviewProductionContext,
) -> Result<(), OrchestratorError> {
    let valid_run = artifact.workflow_run.as_ref().is_some_and(|run| {
        run.id == context.run_id
            && run.repository_id == context.repository_id
            && run.head_repository_id == context.head_repository_id
            && run.head_sha == context.source_sha
            && run.head_branch == plan.default_branch
    });
    if artifact.id != artifact_id
        || artifact.name != expected_name
        || artifact.expired
        || artifact.size_in_bytes == 0
        || !valid_run
        || artifact.digest.as_deref() != Some(format!("sha256:{digest}").as_str())
    {
        return Err(internal("preview_artifact_metadata_identity"));
    }
    Ok(())
}

fn checkpoint(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|_| internal("preview_artifact_deadline"))
}

#[derive(Clone, Deserialize)]
pub(super) struct ArtifactPage {
    pub total_count: usize,
    pub artifacts: Vec<ArtifactRecord>,
}
#[derive(Clone, Deserialize)]
pub(super) struct ArtifactRecord {
    pub id: u64,
    pub name: String,
    pub expired: bool,
    pub size_in_bytes: u64,
    pub digest: Option<String>,
    pub workflow_run: Option<ArtifactSource>,
}
#[derive(Clone, Deserialize)]
pub(super) struct ArtifactSource {
    pub id: u64,
    pub repository_id: u64,
    pub head_repository_id: u64,
    pub head_sha: String,
    pub head_branch: String,
}

#[cfg(test)]
#[path = "preview_artifact_transport_fixture.rs"]
pub(super) mod fixture;

#[cfg(test)]
#[path = "preview_artifact_transport_tests.rs"]
mod tests;
