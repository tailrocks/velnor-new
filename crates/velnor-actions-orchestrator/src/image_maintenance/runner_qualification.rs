// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    RUNNER_QUALIFICATION_SCHEMA, RunnerImageQualification, RunnerImageQualificationImage,
    RunnerImageQualificationImages, parse_runner_image_publication_record,
    validate_runner_image_qualification,
};

use super::{
    WriteRunnerQualificationRequest, runner_admission, runner_profiles::ImageKind, source, validate,
};
use crate::{OrchestratorError, internal::internal};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeResult {
    schema: String,
    source_commit: String,
    runner_lock_sha256: String,
    images: RuntimeImages,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeImages {
    runner: RuntimeImage,
    dind: RuntimeImage,
    resource_probe: RuntimeImage,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeImage {
    runtime_id: String,
    identity_kind: RuntimeIdentityKind,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeIdentityKind {
    Config,
    PlatformManifest,
}

pub(super) fn write(request: &WriteRunnerQualificationRequest) -> Result<(), OrchestratorError> {
    let root = source::checked_root(&request.root)?;
    source::verify_selected_source(&root, &request.source_commit, request.deadline)?;
    if !request.output.is_absolute() || request.output.starts_with(&root) {
        return Err(internal("runner_qualification_output"));
    }
    let qualifier_run_id = number("GITHUB_RUN_ID")?;
    let qualifier_run_attempt = u32::try_from(number("GITHUB_RUN_ATTEMPT")?)
        .map_err(|_| internal("runner_qualification_run_attempt"))?;
    if source::env("GITHUB_SHA")? != request.source_commit
        || source::env("GITHUB_WORKFLOW_SHA")? != request.source_commit
        || source::env("GITHUB_REPOSITORY")? != "tailrocks/velnor-new"
        || source::env("GITHUB_REPOSITORY_ID")? != "1390620900"
        || source::env("GITHUB_EVENT_NAME")? != "push"
        || source::env("GITHUB_REF")? != "refs/heads/main"
        || source::env("GITHUB_WORKFLOW_REF")?
            != "tailrocks/velnor-new/.github/workflows/runner-image-qualification.yml@refs/heads/main"
        || source::env("GITHUB_JOB")? != "qualify-images"
    {
        return Err(internal("runner_qualification_workflow"));
    }
    let bytes = validate::read_limited(&request.runtime_result, 1024 * 1024)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| internal("runner_qualification_runtime_json"))?;
    let json = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| internal("runner_qualification_runtime_json"))?;
    let runtime: RuntimeResult = serde_json::from_value(json)
        .map_err(|_| internal("runner_qualification_runtime_schema"))?;
    let lock = runner_admission::read_exact_lock(&root, &request.source_commit, request.deadline)?;
    let lock_sha = source::hex(&Sha256::digest(&lock));
    if runtime.schema != "velnor.runner-image-runtime-qualification.v1"
        || runtime.source_commit != request.source_commit
        || runtime.runner_lock_sha256 != lock_sha
    {
        return Err(internal("runner_qualification_runtime_identity"));
    }
    let join = ImageJoin {
        request,
        root: &root,
        lock_sha: &lock_sha,
    };
    let proof = RunnerImageQualification {
        schema: RUNNER_QUALIFICATION_SCHEMA.to_owned(),
        source_commit: request.source_commit.clone(),
        source_ref: "refs/heads/main".to_owned(),
        runner_lock_sha256: lock_sha.clone(),
        producer_run_id: request.producer_run_id,
        producer_run_attempt: request.producer_run_attempt,
        qualifier_run_id,
        qualifier_run_attempt,
        qualifier_job_id: request.qualifier_job_id,
        images: RunnerImageQualificationImages {
            runner: join.image(
                "runner",
                ImageKind::Runner,
                &request.runner_archive,
                &request.runner_record,
                &runtime.images.runner,
            )?,
            dind: join.image(
                "dind",
                ImageKind::Dind,
                &request.dind_archive,
                &request.dind_record,
                &runtime.images.dind,
            )?,
            resource_probe: join.image(
                "resource_probe",
                ImageKind::ResourceProbe,
                &request.probe_archive,
                &request.probe_record,
                &runtime.images.resource_probe,
            )?,
        },
    };
    validate_runner_image_qualification(&proof)
        .map_err(|_| internal("runner_qualification_invalid"))?;
    source::verify_selected_source(&root, &request.source_commit, request.deadline)?;
    write_proof(&request.output, &proof)
}

fn number(name: &str) -> Result<u64, OrchestratorError> {
    source::env(name)?
        .parse()
        .map_err(|_| internal("runner_qualification_run_identity"))
}

struct ImageJoin<'a> {
    request: &'a WriteRunnerQualificationRequest,
    root: &'a Path,
    lock_sha: &'a str,
}

impl ImageJoin<'_> {
    fn image(
        &self,
        name: &str,
        kind: ImageKind,
        archive: &Path,
        record: &Path,
        runtime: &RuntimeImage,
    ) -> Result<RunnerImageQualificationImage, OrchestratorError> {
        let bytes = validate::read_limited(record, 1024 * 1024)?;
        let record = parse_runner_image_publication_record(&bytes)
            .map_err(|_| internal("runner_qualification_record"))?;
        if record.image != name
            || record.source.commit != self.request.source_commit
            || record.lock.sha256 != self.lock_sha
            || record.workflow.run_id != self.request.producer_run_id
            || record.workflow.run_attempt != u64::from(self.request.producer_run_attempt)
        {
            return Err(internal("runner_qualification_record_identity"));
        }
        for (path, digest) in &record.source.inputs {
            if source::hash_source_file(self.root, path)? != *digest {
                return Err(internal("runner_qualification_source_input"));
            }
        }
        let graph = runner_admission::inspect_image(
            archive,
            kind,
            &self.request.source_commit,
            self.request.deadline,
        )?
        .evidence;
        if graph.archive_sha256 != record.archive.sha256
            || graph.archive_size_bytes != record.archive.size_bytes
            || graph.archive_index_digest != record.oci.archive_index_digest
            || graph.archive_index_size_bytes != record.oci.archive_index_size_bytes
            || graph.platform_manifest_digest != record.oci.platform_manifest_digest
            || graph.platform_manifest_size_bytes != record.oci.platform_manifest_size_bytes
            || graph.image_configuration_digest != record.oci.image_configuration_digest
            || graph.image_configuration_size_bytes != record.oci.image_configuration_size_bytes
            || graph.layer_digests != record.oci.layer_digests
            || runtime.runtime_id.as_str()
                != match runtime.identity_kind {
                    RuntimeIdentityKind::Config => graph.image_configuration_digest.as_str(),
                    RuntimeIdentityKind::PlatformManifest => {
                        graph.platform_manifest_digest.as_str()
                    }
                }
        {
            return Err(internal("runner_qualification_candidate_mismatch"));
        }
        Ok(RunnerImageQualificationImage {
            archive: record.archive,
            oci: record.oci,
        })
    }
}

fn write_proof(path: &Path, proof: &RunnerImageQualification) -> Result<(), OrchestratorError> {
    let mut bytes =
        serde_json::to_vec(proof).map_err(|_| internal("runner_qualification_serialize"))?;
    bytes.push(b'\n');
    source::write_new_file(path, &bytes)
}
