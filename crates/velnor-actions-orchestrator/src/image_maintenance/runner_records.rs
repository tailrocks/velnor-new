// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, fs};

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    ImageCandidateArchive, ImageCandidateOci, RUNNER_IMAGE_PUBLICATION_SCHEMA,
    RUNNER_PUBLICATION_SOURCE_INPUTS, RunnerImagePublicationLock, RunnerImagePublicationRecord,
    RunnerImagePublicationSource, validate_runner_image_publication_record,
};

use super::{
    RecordRunnerCandidatesRequest, runner_admission, runner_profiles::ImageKind, source, validate,
};
use crate::{OrchestratorError, internal::internal};

pub(super) fn record(request: &RecordRunnerCandidatesRequest) -> Result<(), OrchestratorError> {
    let root = source::checked_root(&request.root)?;
    source::verify_selected_source(&root, &request.source_commit, request.deadline)?;
    if source::env("GITHUB_SHA")? != request.source_commit {
        return Err(internal("runner_candidate_source_identity"));
    }
    let directory = &request.output_directory;
    validate_directory(directory, &root)?;
    let lock_bytes =
        runner_admission::read_exact_lock(&root, &request.source_commit, request.deadline)?;
    let lock = parse_lock(&lock_bytes)?;
    let inputs = RUNNER_PUBLICATION_SOURCE_INPUTS
        .into_iter()
        .map(|path| Ok((path.to_owned(), source::hash_source_file(&root, path)?)))
        .collect::<Result<BTreeMap<_, _>, OrchestratorError>>()?;
    let (workflow, source_ref) = validate::workflow_identity()?;
    let source = RunnerImagePublicationSource {
        commit: request.source_commit.clone(),
        source_ref,
        inputs,
    };
    let mut outputs = Vec::new();
    for (image, kind, path, name) in candidates(request) {
        let output = directory.join(name);
        if output.exists() || fs::symlink_metadata(&output).is_ok() {
            return Err(internal("runner_candidate_output_exists"));
        }
        let graph =
            runner_admission::inspect_image(path, kind, &request.source_commit, request.deadline)?
                .evidence;
        let record = RunnerImagePublicationRecord {
            schema: RUNNER_IMAGE_PUBLICATION_SCHEMA.to_owned(),
            image: image.to_owned(),
            source: source.clone(),
            workflow: workflow.clone(),
            platform: "linux/amd64".to_owned(),
            archive: ImageCandidateArchive {
                sha256: graph.archive_sha256,
                size_bytes: graph.archive_size_bytes,
            },
            oci: ImageCandidateOci {
                archive_index_digest: graph.archive_index_digest,
                archive_index_size_bytes: graph.archive_index_size_bytes,
                platform_manifest_digest: graph.platform_manifest_digest,
                platform_manifest_size_bytes: graph.platform_manifest_size_bytes,
                image_configuration_digest: graph.image_configuration_digest,
                image_configuration_size_bytes: graph.image_configuration_size_bytes,
                layer_digests: graph.layer_digests,
            },
            lock: lock.clone(),
        };
        validate_runner_image_publication_record(&record)
            .map_err(|_| internal("runner_candidate_record_invalid"))?;
        let mut bytes = serde_json::to_vec(&record)
            .map_err(|_| internal("runner_candidate_record_encoding"))?;
        bytes.push(b'\n');
        outputs.push((output, bytes));
    }
    source::verify_selected_source(&root, &request.source_commit, request.deadline)?;
    if runner_admission::read_exact_lock(&root, &request.source_commit, request.deadline)?
        != lock_bytes
    {
        return Err(internal("runner_candidate_lock_changed"));
    }
    for (path, bytes) in outputs {
        source::write_new_file(&path, &bytes)?;
    }
    Ok(())
}

fn parse_lock(lock_bytes: &[u8]) -> Result<RunnerImagePublicationLock, OrchestratorError> {
    validate::validate_runner_lock(lock_bytes)?;
    let lock_text =
        std::str::from_utf8(lock_bytes).map_err(|_| internal("runner_candidate_lock"))?;
    let lock_value: toml::Value =
        toml::from_str(lock_text).map_err(|_| internal("runner_candidate_lock"))?;
    let version = |name: &str| -> Result<String, OrchestratorError> {
        lock_value
            .get(name)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| internal("runner_candidate_lock"))
    };
    Ok(RunnerImagePublicationLock {
        sha256: source::hex(&Sha256::digest(lock_bytes)),
        runner_version: version("runner_version")?,
        docker_version: version("docker_version")?,
        buildx_version: version("buildx_version")?,
        compose_version: version("compose_version")?,
        node_version: version("node_version")?,
    })
}

fn validate_directory(
    directory: &std::path::Path,
    root: &std::path::Path,
) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(directory)
        .map_err(|_| internal("runner_candidate_output_directory"))?;
    if !directory.is_absolute()
        || !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || directory
            .canonicalize()
            .map_err(|_| internal("runner_candidate_output_directory"))?
            .starts_with(root)
    {
        return Err(internal("runner_candidate_output_directory"));
    }
    Ok(())
}

fn candidates(
    request: &RecordRunnerCandidatesRequest,
) -> [(&'static str, ImageKind, &std::path::Path, &'static str); 3] {
    [
        (
            "runner",
            ImageKind::Runner,
            &request.runner_archive,
            "runner-image-publication.json",
        ),
        (
            "dind",
            ImageKind::Dind,
            &request.dind_archive,
            "dind-image-publication.json",
        ),
        (
            "resource_probe",
            ImageKind::ResourceProbe,
            &request.probe_archive,
            "resource-probe-image-publication.json",
        ),
    ]
}
