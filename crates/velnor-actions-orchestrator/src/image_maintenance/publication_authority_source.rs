// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use velnor_actions_contract::config::analyze_dockerfile;
use velnor_actions_contract::{
    IMAGE_CANDIDATE_SOURCE_INPUTS, RUNNER_PUBLICATION_SOURCE_INPUTS,
    RunnerPublicationDockerfileContext, runner_publication_dockerfile_context,
};

use crate::{OrchestratorError, internal::internal};

use super::{CandidateRecord, RunnerLockVersions, hex, image_name};

pub(super) fn verify_source_inputs(
    candidate: &CandidateRecord,
    source_files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), OrchestratorError> {
    if source_files.len() != source_paths(candidate.profile).len()
        || source_files.keys().ne(candidate.source_inputs.keys())
    {
        return Err(internal("image_publication_source_inventory"));
    }
    for (path, bytes) in source_files {
        if candidate.source_inputs.get(path).map(String::as_str)
            != Some(hex(&Sha256::digest(bytes)).as_str())
        {
            return Err(internal("image_publication_source_digest"));
        }
    }
    let lock_bytes = source_files
        .get(".velnor/runner.lock")
        .ok_or_else(|| internal("image_publication_lock_missing"))?;
    let lock: RunnerLockVersions = toml::from_str(
        std::str::from_utf8(lock_bytes).map_err(|_| internal("image_publication_lock_utf8"))?,
    )
    .map_err(|_| internal("image_publication_lock_parse"))?;
    if [
        &lock.runner,
        &lock.docker,
        &lock.buildx,
        &lock.compose,
        &lock.node,
    ]
    .iter()
    .any(|version| !velnor_actions_contract::config::is_exact_semver(version))
    {
        return Err(internal("image_publication_lock_versions"));
    }
    if let Some(expected) = &candidate.lock
        && (expected.sha256 != hex(&Sha256::digest(lock_bytes))
            || expected.runner_version != lock.runner
            || expected.docker_version != lock.docker
            || expected.buildx_version != lock.buildx
            || expected.compose_version != lock.compose
            || expected.node_version != lock.node)
    {
        return Err(internal("image_publication_runner_lock_identity"));
    }
    if let Some(builder) = &candidate.buildx_builder {
        let version = super::validate::buildx_release_version(builder)
            .ok_or_else(|| internal("image_publication_job_buildx_identity"))?;
        if version != lock.buildx {
            return Err(internal("image_publication_job_buildx_identity"));
        }
    }
    if let Some(context) = runner_publication_dockerfile_context(image_name(candidate.profile)) {
        verify_dockerfile_context(source_files, context)?;
    }
    Ok(())
}

pub(super) fn source_paths(
    profile: super::ImagePublicationProfile,
) -> &'static [&'static str] {
    match profile {
        super::ImagePublicationProfile::Job => &IMAGE_CANDIDATE_SOURCE_INPUTS,
        super::ImagePublicationProfile::Runner
        | super::ImagePublicationProfile::Dind
        | super::ImagePublicationProfile::ResourceProbe => &RUNNER_PUBLICATION_SOURCE_INPUTS,
    }
}

fn verify_dockerfile_context(
    source_files: &BTreeMap<String, Vec<u8>>,
    context: RunnerPublicationDockerfileContext,
) -> Result<(), OrchestratorError> {
    let bytes = source_files
        .get(context.dockerfile)
        .ok_or_else(|| internal("image_publication_source_dockerfile_missing"))?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| internal("image_publication_source_dockerfile_utf8"))?;
    let facts = analyze_dockerfile(text)
        .map_err(|_| internal("image_publication_source_dockerfile"))?;
    if !context_sources_match(&facts.context_sources, context) {
        return Err(internal("image_publication_source_context"));
    }
    Ok(())
}

fn context_sources_match(
    actual: &BTreeSet<String>,
    context: RunnerPublicationDockerfileContext,
) -> bool {
    let expected_len = context.tracked_sources.len() + context.generated_sources.len();
    actual.len() == expected_len
        && context
            .tracked_sources
            .iter()
            .chain(context.generated_sources)
            .all(|source| actual.contains(*source))
}
