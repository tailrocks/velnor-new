//! Current-attempt, ID-addressed acquisition of unsigned native preview leaves.
//!
//! The artifact API has no attempt field. Preview assembly derives the complete
//! pair union from the authenticated current-attempt job inventory and exact
//! deterministic artifact names; stable assembly retains literal upload outputs.
//! Timestamps never serve as attempt evidence. ZIP admission follows separately,
//! and acquired bytes alone are not an admitted payload.

use super::artifact_zip::{ZipProfile, leaf_union_compressed_bound};
use super::github::ReleaseArtifactApi;
use std::{fs::File, path::Path};
#[path = "preview_artifact_transport_file.rs"]
mod archive_file;
use crate::{OrchestratorError, internal::internal};
use archive_file::SealedArchive;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::PackageReleasePlan;
use velnor_actions_contract::preview_production::{
    PayloadArtifactRef, ReleaseProductionContext, ReleaseProductionRequest, ReleaseProductionStage,
    release_payload_artifact_name,
};
use velnor_actions_mise::CheckDeadline;

#[path = "preview_artifact_transport_aggregate.rs"]
mod aggregate;
pub(crate) use aggregate::acquire_release_aggregate;

const MAX_ARTIFACTS: usize = 1000;

/// Authenticated service archive bytes, still requiring closed ZIP/leaf admission.
pub(crate) struct AdmittedReleaseArtifact {
    pub reference: PayloadArtifactRef,
    pub archive: SealedArchive,
}

/// Acquire exactly the admitted assembly union under one cumulative budget.
/// Stable uses literal references; preview derives them from GitHub's current
/// attempt artifact and successful job inventories. This reauthenticates GitHub.
pub(crate) fn acquire_release_artifacts(
    request: &ReleaseProductionRequest,
    directory: &Path,
    deadline: CheckDeadline,
    mut consume: impl FnMut(AdmittedReleaseArtifact) -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    require_consumer_environment(ReleaseConsumer::Assembler, request.plan.channel)?;
    let api = ReleaseArtifactApi::new(&request.plan.repository);
    deadline
        .scope(|| acquire(&api, request, directory, deadline, &mut consume))
        .map_err(|_| internal("preview_artifact_deadline"))?
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReleaseConsumer {
    Assembler,
    Signer,
}
impl ReleaseConsumer {
    pub(in crate::package_release_publish) fn job_name(
        self,
        channel: velnor_actions_contract::package_release::PackageReleaseChannel,
    ) -> &'static str {
        match self {
            Self::Assembler => channel.assemble_job_id(),
            Self::Signer => channel.signer_job_id(),
        }
    }
}
fn require_consumer_environment(
    consumer: ReleaseConsumer,
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
) -> Result<(), OrchestratorError> {
    validate_consumer_environment(
        consumer,
        channel,
        std::env::var("GITHUB_JOB").ok().as_deref(),
    )?;
    // This new read capability must not fall back to ambient cached gh login.
    let token_present = std::env::var("GH_TOKEN").is_ok_and(|token| !token.trim().is_empty());
    if !token_present {
        return Err(internal("preview_artifact_read_token_missing"));
    }
    Ok(())
}
fn validate_consumer_environment(
    consumer: ReleaseConsumer,
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
    actual: Option<&str>,
) -> Result<(), OrchestratorError> {
    if actual != Some(consumer.job_name(channel)) {
        return Err(internal("preview_artifact_consumer_environment"));
    }
    Ok(())
}

pub(super) trait ArtifactReadApi {
    fn authenticate(
        &self,
        plan: &PackageReleasePlan,
        context: &ReleaseProductionContext,
        consumer: ReleaseConsumer,
    ) -> Result<(), OrchestratorError>;
    fn page(&self, run: u64, page: usize) -> Result<ArtifactPage, OrchestratorError>;
    fn jobs(&self, run: u64, attempt: u32) -> Result<Vec<JobRecord>, OrchestratorError>;
    fn download(
        &self,
        id: u64,
        size: u64,
        output: &mut File,
        deadline: CheckDeadline,
    ) -> Result<(), OrchestratorError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JobRecord {
    pub run_id: u64,
    pub name: String,
    pub head_sha: Option<String>,
    pub status: Option<String>,
    pub conclusion: Option<String>,
}

fn acquire(
    api: &impl ArtifactReadApi,
    request: &ReleaseProductionRequest,
    directory: &Path,
    deadline: CheckDeadline,
    consume: &mut impl FnMut(AdmittedReleaseArtifact) -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    let preview = request.plan.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Preview;
    if !preview {
        request
            .validate()
            .map_err(|_| internal("preview_artifact_request"))?;
    }
    let ReleaseProductionStage::Assemble { inputs } = &request.stage else {
        return Err(internal("preview_artifact_assembly_required"));
    };
    checkpoint(deadline)?;
    api.authenticate(&request.plan, &request.context, ReleaseConsumer::Assembler)?;
    let inventory = inventory(api, request.context.run_id, deadline)?;
    let discovered;
    let inputs = if request.plan.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Preview
    {
        let jobs = api.jobs(request.context.run_id, request.context.run_attempt)?;
        discovered = discover_preview_inputs(request, &inventory, &jobs)?;
        let mut completed = request.clone();
        let ReleaseProductionStage::Assemble { inputs } = &mut completed.stage else {
            return Err(internal("preview_artifact_assembly_required"));
        };
        inputs.clone_from(&discovered);
        completed
            .validate()
            .map_err(|_| internal("preview_artifact_request"))?;
        discovered.as_slice()
    } else {
        inputs.as_slice()
    };
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
            archive.destination_file(),
            deadline,
        )?;
        archive.seal(deadline)?;
        consume(AdmittedReleaseArtifact {
            reference: reference.clone(),
            archive,
        })?;
        checkpoint(deadline)?;
    }
    api.authenticate(&request.plan, &request.context, ReleaseConsumer::Assembler)?;
    checkpoint(deadline)?;
    Ok(())
}

fn discover_preview_inputs(
    request: &ReleaseProductionRequest,
    inventory: &BTreeMap<u64, ArtifactRecord>,
    jobs: &[JobRecord],
) -> Result<Vec<PayloadArtifactRef>, OrchestratorError> {
    let expected_pairs =
        velnor_actions_contract::preview_production::expected_release_payload_pairs(&request.plan)
            .map_err(|_| internal("preview_artifact_plan"))?;
    let mut expected_jobs = BTreeSet::new();
    let mut inputs = Vec::with_capacity(expected_pairs.len());
    let mut ids = BTreeSet::new();
    for pair in &expected_pairs {
        let job = format!("Build {} / {}", pair.product_id, pair.target);
        expected_jobs.insert(job.clone());
        let name = release_payload_artifact_name(&request.plan, pair, &request.context)
            .map_err(|_| internal("preview_artifact_name"))?;
        let matches = inventory
            .values()
            .filter(|artifact| artifact.name == name)
            .collect::<Vec<_>>();
        let [artifact] = matches.as_slice() else {
            return Err(internal("preview_artifact_payload_inventory"));
        };
        let digest = artifact
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or_else(|| internal("preview_artifact_payload_digest"))?;
        if artifact.id == 0 || !ids.insert(artifact.id) || !lower_sha256(digest) {
            return Err(internal("preview_artifact_payload_identity"));
        }
        inputs.push(PayloadArtifactRef {
            pair: pair.clone(),
            artifact_id: artifact.id,
            sha256: digest.to_owned(),
        });
    }
    let namespace = format!(
        "velnor-release-payload-r{}-a{}-",
        request.context.run_id, request.context.run_attempt
    );
    if inventory
        .values()
        .any(|artifact| artifact.name.starts_with(&namespace) && !ids.contains(&artifact.id))
    {
        return Err(internal("preview_artifact_unexpected_payload"));
    }
    let mut observed_jobs = BTreeSet::new();
    for job in jobs.iter().filter(|job| job.name.starts_with("Build ")) {
        if job.run_id != request.context.run_id
            || job.head_sha.as_deref() != Some(request.context.workflow_authority_sha.as_str())
            || job.status.as_deref() != Some("completed")
            || job.conclusion.as_deref() != Some("success")
            || !expected_jobs.contains(&job.name)
            || !observed_jobs.insert(job.name.clone())
        {
            return Err(internal("preview_artifact_payload_job_inventory"));
        }
    }
    if observed_jobs != expected_jobs {
        return Err(internal("preview_artifact_payload_job_inventory"));
    }
    Ok(inputs)
}

fn lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    request: &ReleaseProductionRequest,
) -> Result<(), OrchestratorError> {
    let expected_name =
        release_payload_artifact_name(&request.plan, &reference.pair, &request.context)
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
    context: &ReleaseProductionContext,
) -> Result<(), OrchestratorError> {
    let valid_run = artifact.workflow_run.as_ref().is_some_and(|run| {
        run.id == context.run_id
            && run.repository_id == context.repository_id
            && run.head_repository_id == context.head_repository_id
            && run.head_sha == context.workflow_authority_sha
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

#[cfg(test)]
#[path = "preview_artifact_transport_environment_tests.rs"]
mod environment_tests;
