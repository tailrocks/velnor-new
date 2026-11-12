// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{ImagePublicationProfile, PublishImageRequest, PublishRunnerImagesRequest};
use super::{PublishedCandidate, authority, crane, family, github, qualification};
use crate::{OrchestratorError, internal::internal};
use std::{os::unix::fs::PermissionsExt, path::Path};
use velnor_actions_contract::RUNNER_IMAGE_FAMILY_ARTIFACT_NAMES;
use velnor_actions_contract::runner_image_manifest::{
    ImageEntries, ImageEntry, ResourceProbeProfile, RunnerImageManifest, TrustedSigningIdentity,
    Versions,
};

const PROFILES: [ImagePublicationProfile; 3] = [
    ImagePublicationProfile::Runner,
    ImagePublicationProfile::Dind,
    ImagePublicationProfile::ResourceProbe,
];

pub(super) fn run(
    request: &PublishRunnerImagesRequest,
    verify_only: bool,
) -> Result<(), OrchestratorError> {
    super::validate_scratch(&request.private_scratch)?;
    if !request.assets_dir.is_absolute() || !request.manifest_output.is_absolute() {
        return Err(internal("runner_publication_paths"));
    }
    let journal = if verify_only {
        request.private_scratch.clone()
    } else {
        tempfile::Builder::new()
            .prefix("velnor-runner-publication-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(&request.private_scratch)
            .map_err(|_| internal("runner_publication_journal"))?
            .keep()
    };
    let requests = PROFILES.map(|profile| member(request, profile, &journal));
    if let Err(error) = preflight(request, &requests) {
        if !verify_only {
            std::fs::remove_dir(&journal)
                .map_err(|_| internal("runner_publication_journal_cleanup"))?;
        }
        return Err(error);
    }
    let mut published = Vec::new();
    for member in &requests {
        published.push(if verify_only {
            recheck(member)?
        } else {
            super::publish_verified(member)?
        });
    }
    let manifest = manifest(request, &published)?;
    let token =
        std::env::var("GH_TOKEN").map_err(|_| internal("image_publication_token_missing"))?;
    if github::ci::main_sha(&token, request.deadline)? != request.source_commit {
        return Err(internal("runner_publication_main_changed"));
    }
    if verify_only {
        let bytes = super::super::validate::read_limited(&request.manifest_output, 1024 * 1024)?;
        let value = velnor_actions_contract::parse_strict_json(
            std::str::from_utf8(&bytes).map_err(|_| internal("runner_publication_manifest"))?,
        )
        .map_err(|_| internal("runner_publication_manifest"))?;
        let actual: RunnerImageManifest =
            serde_json::from_value(value).map_err(|_| internal("runner_publication_manifest"))?;
        if serde_json::to_value(&actual).map_err(|_| internal("runner_publication_manifest"))?
            != serde_json::to_value(&manifest)
                .map_err(|_| internal("runner_publication_manifest"))?
        {
            return Err(internal("runner_publication_manifest_changed"));
        }
        return Ok(());
    }
    let mut bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| internal("runner_publication_manifest"))?;
    bytes.push(b'\n');
    super::super::source::write_new_file(&request.manifest_output, &bytes)
}

fn member(
    request: &PublishRunnerImagesRequest,
    profile: ImagePublicationProfile,
    journal: &Path,
) -> PublishImageRequest {
    PublishImageRequest {
        profile,
        source_commit: request.source_commit.clone(),
        producer_run_id: request.producer_run_id,
        producer_run_attempt: request.producer_run_attempt,
        artifact_id: request.artifact_id,
        archive_path: request.assets_dir.join(authority::archive_name(profile)),
        record_path: request.assets_dir.join(authority::record_name(profile)),
        qualification_proof: Some(request.qualification_proof.clone()),
        job_attestation_run_id: None,
        job_attestation_run_attempt: None,
        private_scratch: request.private_scratch.clone(),
        receipt_output: journal.join(format!("{}.jsonl", authority::image_name(profile))),
        deadline: request.deadline,
    }
}

fn preflight(
    request: &PublishRunnerImagesRequest,
    members: &[PublishImageRequest; 3],
) -> Result<(), OrchestratorError> {
    for name in RUNNER_IMAGE_FAMILY_ARTIFACT_NAMES {
        let metadata = std::fs::symlink_metadata(request.assets_dir.join(name))
            .map_err(|_| internal("runner_publication_asset_inventory"))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(internal("runner_publication_asset_inventory"));
        }
    }
    let token =
        std::env::var("GH_TOKEN").map_err(|_| internal("image_publication_token_missing"))?;
    if github::ci::main_sha(&token, request.deadline)? != request.source_commit {
        return Err(internal("runner_publication_not_current_main"));
    }
    let records = members
        .iter()
        .map(authority::load_candidate)
        .collect::<Result<Vec<_>, _>>()?;
    let first = &records[0];
    let probe = first
        .provenance
        .as_ref()
        .and_then(|value| value.named_contexts.get("resource_probe"))
        .ok_or_else(|| internal("runner_publication_context"))?;
    for (member, candidate) in members.iter().zip(&records) {
        if candidate.source_commit != first.source_commit
            || candidate.source_ref != first.source_ref
            || candidate.source_inputs != first.source_inputs
            || candidate.lock != first.lock
            || candidate
                .provenance
                .as_ref()
                .and_then(|value| value.named_contexts.get("resource_probe"))
                != Some(probe)
        {
            return Err(internal("runner_publication_family_identity"));
        }
        qualification::load(member, candidate)?
            .ok_or_else(|| internal("runner_publication_qualification"))?;
    }
    Ok(())
}

fn recheck(request: &PublishImageRequest) -> Result<PublishedCandidate, OrchestratorError> {
    let candidate = authority::load_candidate(request)?;
    authority::verify_archive_bytes(request, &candidate)?;
    let proof = qualification::load(request, &candidate)?;
    let (producer, sources) =
        github::authenticate(request, &candidate, proof.as_ref(), request.deadline)?;
    let bound = family::load(request, &candidate, &sources)?
        .ok_or_else(|| internal("runner_publication_provenance"))?;
    let publication = crane::verify_published(
        authority::repository(request.profile),
        &authority::tag(&request.source_commit),
        &candidate.oci,
        request.deadline,
    )?;
    bound.revalidate()?;
    github::revalidate_before_publish(request, &producer, request.deadline)?;
    Ok(PublishedCandidate {
        candidate,
        producer,
        publication,
        qualification_sha256: proof.map(|value| value.sha256),
        build_source: Some(bound.source().clone()),
    })
}

fn manifest(
    request: &PublishRunnerImagesRequest,
    published: &[PublishedCandidate],
) -> Result<RunnerImageManifest, OrchestratorError> {
    if published.len() != 3 {
        return Err(internal("runner_publication_family_count"));
    }
    let first = &published[0];
    let lock = first
        .candidate
        .lock
        .as_ref()
        .ok_or_else(|| internal("runner_publication_lock"))?;
    let proof = first
        .qualification_sha256
        .as_ref()
        .ok_or_else(|| internal("runner_publication_qualification"))?;
    let probe = first
        .candidate
        .provenance
        .as_ref()
        .and_then(|value| value.named_contexts.get("resource_probe"))
        .ok_or_else(|| internal("runner_publication_context"))?;
    for (item, profile) in published.iter().zip(PROFILES) {
        if item.candidate.profile != profile
            || item.candidate.source_commit != request.source_commit
            || item.candidate.source_inputs != first.candidate.source_inputs
            || item.candidate.lock.as_ref() != Some(lock)
            || item.producer.head_sha != request.source_commit
            || item.producer.main_sha != request.source_commit
            || item.producer.ci_run_id != request.producer_run_id
            || item.producer.ci_run_attempt != request.producer_run_attempt
            || item.qualification_sha256.as_ref() != Some(proof)
            || item
                .candidate
                .provenance
                .as_ref()
                .and_then(|value| value.named_contexts.get("resource_probe"))
                != Some(probe)
            || item.publication.digest != item.candidate.oci.platform_manifest_digest
        {
            return Err(internal("runner_publication_verified_family"));
        }
    }
    Ok(RunnerImageManifest { schema_version: 2, repository: "tailrocks/velnor-new".to_owned(), source_ref: "refs/heads/main".to_owned(),
        source_commit: request.source_commit.clone(), runner_lock_sha256: lock.sha256.clone(),
        signer_workflow: ".github/workflows/product-release-images.yml".to_owned(), signer_ref: "refs/heads/main".to_owned(), workflow_authority_sha: request.source_commit.clone(),
        trusted_signing_identity: TrustedSigningIdentity { oidc_issuer: "https://token.actions.githubusercontent.com".to_owned(), certificate_identity: "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main".to_owned() },
        platform: "linux/amd64".to_owned(), versions: Versions { runner: lock.runner_version.clone(), docker: lock.docker_version.clone(), buildx: lock.buildx_version.clone(), compose: lock.compose_version.clone(), node: lock.node_version.clone() },
        images: ImageEntries { runner: image(&published[0])?, dind: image(&published[1])?, resource_probe: image(&published[2])? },
        resource_probe: ResourceProbeProfile { protocol_version: 1, image_user: "65532:65532".to_owned(), entrypoint: "/velnor/resource-probe".to_owned(), working_directory: "/".to_owned() } })
}

fn image(item: &PublishedCandidate) -> Result<ImageEntry, OrchestratorError> {
    Ok(ImageEntry {
        repository: authority::repository(item.candidate.profile).to_owned(),
        tag: authority::tag(&item.candidate.source_commit),
        registry_digest: item.publication.digest.clone(),
        platform_manifest_digest: item.candidate.oci.platform_manifest_digest.clone(),
        config_digest: item.candidate.oci.image_configuration_digest.clone(),
        build_source: item
            .build_source
            .clone()
            .ok_or_else(|| internal("runner_publication_source"))?,
    })
}
