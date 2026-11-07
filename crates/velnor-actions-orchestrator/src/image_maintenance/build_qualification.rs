// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use velnor_actions_contract::{
    IMAGE_QUALIFICATION_INPUTS, IMAGE_QUALIFICATION_SCHEMA, ImageCandidateArchive,
    ImageCandidateBuild, ImageCandidateOci, NativeImageQualificationRecord,
    serialize_native_image_qualification_record,
};
use velnor_actions_mise::ImageCandidateCommand;

use super::super::{BuildJobCandidateRequest, QualifyNativeImageRequest, source};
use super::candidate::{self, CandidatePayload};
use super::support;
use crate::{OrchestratorError, internal::internal};

pub(super) fn qualify(request: &QualifyNativeImageRequest) -> Result<(), OrchestratorError> {
    let request = BuildJobCandidateRequest {
        root: request.root.clone(),
        source_commit: request.source_commit.clone(),
        runner_temp: request.runner_temp.clone(),
        archive_path: request
            .runner_temp
            .join("native-image-qualification.oci.tar"),
        record_path: request.runner_temp.join("native-image-qualification.json"),
        source_closure_path: request.source_closure_path.clone(),
        deadline: request.deadline,
    };
    request
        .deadline
        .remaining()
        .map_err(|_| internal("native_qualification_deadline"))?;
    let inputs = candidate::prepare_inputs(&request)?;
    let container_id = super::super::qualification_service::verify(&inputs.root, &request.runner_temp, request.deadline)?;
    let base_layout = super::qualification_base::prepare(&request.runner_temp, request.deadline)?;
    let sources = IMAGE_QUALIFICATION_INPUTS
        .into_iter()
        .map(|path| {
            Ok((
                path.to_owned(),
                source::hash_source_file(&inputs.root, path)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, OrchestratorError>>()?;
    super::super::validate::qualification_identity(&request.source_commit)?;
    let binary = request
        .runner_temp
        .join("velnor-native-image-qualification/target/release/velnor-native-image-qualification");
    let binary_sha256 = support::hash_regular(&binary, support::MAX_BUILDX_BYTES)?;
    let context = candidate::prepare_context(
        &inputs,
        CandidatePayload::Qualification {
            binary: &binary,
            binary_sha256: &binary_sha256,
        },
    )?;
    let builder = candidate::run_build(
        &context,
        &inputs.lock,
        &request.source_commit,
        candidate::DockerTarget::Qualification { container_id: &container_id, base_layout: &base_layout },
        request.deadline,
    )?;
    let graph = verify_runtime(&context, &container_id, &base_layout, &inputs.lock.docker_version, request.deadline)?;
    super::super::qualification_service::verify(&inputs.root, &request.runner_temp, request.deadline)?;
    let record = qualification_record(&request, inputs, sources, binary_sha256, builder, graph);
    let bytes = serialize_native_image_qualification_record(&record)
        .map_err(|_| internal("native_qualification_record"))?;
    candidate::publish_bytes(&context, &request, &bytes)
}

fn qualification_record(
    request: &BuildJobCandidateRequest,
    inputs: candidate::CandidateInputs,
    sources: BTreeMap<String, String>,
    binary_sha256: String,
    builder: String,
    graph: velnor_archive_guard::owned_archive_guard::oci_candidate::OciCandidateEvidence,
) -> NativeImageQualificationRecord {
    NativeImageQualificationRecord {
        schema: IMAGE_QUALIFICATION_SCHEMA.to_owned(),
        source_commit: request.source_commit.clone(),
        source_ref: inputs.source_ref,
        source_inputs: sources,
        binary_sha256,
        workflow: inputs.workflow,
        build: ImageCandidateBuild {
            builder,
            platform: "linux/amd64".to_owned(),
            arguments: Vec::new(),
            exporter: "oci".to_owned(),
            provenance: "disabled".to_owned(),
            compression: "gzip".to_owned(),
            force_compression: true,
        },
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
    }
}

fn verify_runtime(
    context: &candidate::BuildContext,
    container_id: &str,
    base_layout: &std::path::Path,
    docker_version: &str,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<
    velnor_archive_guard::owned_archive_guard::oci_candidate::OciCandidateEvidence,
    OrchestratorError,
> {
    let inspection = velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_oci_candidate_with_config(&context.archive)
        .map_err(|_| internal("native_qualification_oci_invalid"))?;
    validate_configuration(&inspection.configuration)?;
    let daemon = ImageCandidateCommand::qualification_daemon_version(&context.docker_config)
        .map_err(|_| internal("native_qualification_daemon_command"))?;
    let target = candidate::DockerTarget::Qualification { container_id, base_layout };
    let daemon = candidate::docker_target(daemon, &target)?;
    let actual = ImageCandidateCommand::run(&daemon, deadline)
        .map_err(|_| internal("native_qualification_daemon"))?;
    if actual
        .stdout_text("docker")
        .map_err(|_| internal("native_qualification_daemon_version"))?
        .trim()
        != docker_version
    {
        return Err(internal("native_qualification_daemon_version"));
    }
    let load =
        ImageCandidateCommand::load_qualification_image(&context.archive, &context.docker_config)
            .map_err(|_| internal("native_qualification_load_command"))?;
    let load = candidate::docker_target(load, &target)?;
    ImageCandidateCommand::run(&load, deadline)
        .map_err(|_| internal("native_qualification_load"))?;
    let run = ImageCandidateCommand::run_qualification_image(
        &inspection.evidence.platform_manifest_digest,
        &context.docker_config,
    )
    .map_err(|_| internal("native_qualification_run_command"))?;
    let run = candidate::docker_target(run, &target)?;
    let output = ImageCandidateCommand::run(&run, deadline)
        .map_err(|_| internal("native_qualification_run"))?;
    if output
        .stdout_text("docker")
        .map_err(|_| internal("native_qualification_output"))?
        .trim()
        != "velnor-native-image-qualification:547"
    {
        return Err(internal("native_qualification_output"));
    }
    Ok(inspection.evidence)
}

fn validate_configuration(
    configuration: &velnor_archive_guard::owned_archive_guard::oci_candidate::OciImageConfiguration,
) -> Result<(), OrchestratorError> {
    if configuration.user.as_deref() != Some("65532:65532")
        || configuration.entrypoint.as_deref() != Some(&["/fixture".to_owned()][..])
        || configuration.cmd.as_deref().is_some_and(|cmd| !cmd.is_empty())
    {
        return Err(internal("native_qualification_oci_configuration"));
    }
    Ok(())
}
