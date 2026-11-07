// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use tempfile::{Builder, TempDir};
use velnor_actions_contract::{
    ImageCandidateArchive, ImageCandidateBuild, ImageCandidateOci, ImageCandidateRecord,
    ImageCandidateSource, parse_image_candidate_source_closure, serialize_image_candidate_record,
};
use velnor_actions_mise::{CheckDeadline, ImageCandidateCommand};

use crate::{OrchestratorError, internal::internal};

use super::super::validate::{
    BuildxLock, buildx_release_version, read_limited, read_lock, workflow_identity,
};
use super::super::{BuildJobCandidateRequest, source};
use super::candidate_files::{copy_source_file, remove_our_archive_link, write_staged};
use super::support;

pub(super) struct CandidateInputs {
    pub(super) root: PathBuf,
    pub(super) output_parent: PathBuf,
    pub(super) plugin: PathBuf,
    pub(super) source_inputs: BTreeMap<String, String>,
    pub(super) source_ref: String,
    pub(super) workflow: velnor_actions_contract::ImageCandidateWorkflow,
    pub(super) lock: BuildxLock,
}

pub(super) struct BuildContext {
    pub(super) directory: TempDir,
    pub(super) docker_config: PathBuf,
    plugin: PathBuf,
    pub(super) archive: PathBuf,
}

#[derive(Clone, Copy)]
pub(super) enum DockerTarget<'a> {
    Production,
    Qualification { container_id: &'a str, base_layout: &'a Path },
}

pub(super) fn build_job_candidate(
    request: &BuildJobCandidateRequest,
) -> Result<(), OrchestratorError> {
    request
        .deadline
        .remaining()
        .map_err(|_| internal("image_candidate_deadline"))?;
    let inputs = prepare_inputs(request)?;
    let context = prepare_context(&inputs, CandidatePayload::Production)?;
    let builder = run_build(
        &context,
        &inputs.lock,
        &request.source_commit,
        DockerTarget::Production,
        request.deadline,
    )?;
    let record = build_record(request, inputs, &context.archive, builder)?;
    publish_candidate(&context, request, &record)
}

pub(super) fn prepare_inputs(
    request: &BuildJobCandidateRequest,
) -> Result<CandidateInputs, OrchestratorError> {
    let root = source::checked_root(&request.root)?;
    source::ensure_clean_checkout(&root, request.deadline)?;
    let head = source::git_text(
        &root,
        &velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()]),
        request.deadline,
    )?;
    if head.trim() != request.source_commit || source::env("GITHUB_SHA")? != request.source_commit {
        return Err(internal("image_candidate_source_identity"));
    }
    verify_closure(&root, &request.source_commit, &request.source_closure_path)?;
    let check = ImageCandidateCommand::check_job_source(&root)
        .map_err(|_| internal("image_candidate_source_command"))?;
    ImageCandidateCommand::run(&check, request.deadline)
        .map_err(|_| internal("image_candidate_source_check"))?;
    let temp = support::owned_directory(&request.runner_temp)?;
    if temp.starts_with(&root) {
        return Err(internal("image_candidate_temp_inside_source"));
    }
    let plugin = temp.join(".velnor-job-image-candidate/docker-config/cli-plugins/docker-buildx");
    let lock = read_lock(&root)?;
    if support::hash_regular(&plugin, support::MAX_BUILDX_BYTES)? != lock.sha256 {
        return Err(internal("image_candidate_buildx_not_installed"));
    }
    let output_parent = validate_outputs(&root, &request.archive_path, &request.record_path)?;
    let source_inputs = source::source_inputs(&root)?;
    let (workflow, source_ref) = workflow_identity()?;
    Ok(CandidateInputs {
        root,
        output_parent,
        plugin,
        source_inputs,
        source_ref,
        workflow,
        lock,
    })
}

#[derive(Clone, Copy)]
pub(super) enum CandidatePayload<'a> {
    Production,
    Qualification {
        binary: &'a Path,
        binary_sha256: &'a str,
    },
}

pub(super) fn prepare_context(
    inputs: &CandidateInputs,
    payload: CandidatePayload<'_>,
) -> Result<BuildContext, OrchestratorError> {
    let directory = Builder::new()
        .prefix(".velnor-job-image-candidate-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&inputs.output_parent)
        .map_err(|error| {
            OrchestratorError::io(
                inputs.output_parent.display().to_string(),
                error.to_string(),
            )
        })?;
    let dockerfile_path = match &payload {
        CandidatePayload::Production => "images/job/ubuntu-26.04/Dockerfile",
        CandidatePayload::Qualification { .. } => {
            "crates/velnor-actions-cli/tests/fixtures/native-image-qualification/Dockerfile"
        }
    };
    let dockerfile = inputs.root.join(dockerfile_path);
    let expected = source::hash_source_file(&inputs.root, dockerfile_path)?;
    copy_source_file(&dockerfile, &directory.path().join("Dockerfile"), &expected)?;
    if let CandidatePayload::Qualification {
        binary,
        binary_sha256,
    } = payload
    {
        if support::hash_regular(binary, support::MAX_BUILDX_BYTES)? != binary_sha256 {
            return Err(internal("native_qualification_binary_changed"));
        }
        let output = directory.path().join("fixture");
        fs::copy(binary, &output).map_err(|error| {
            OrchestratorError::io(output.display().to_string(), error.to_string())
        })?;
        if support::hash_regular(&output, support::MAX_BUILDX_BYTES)? != binary_sha256 {
            return Err(internal("native_qualification_binary_copy"));
        }
    }
    let docker_config = directory.path().join(support::DOCKER_ENV);
    let plugin_directory =
        support::private_subdirectory(directory.path(), "docker-config/cli-plugins")?;
    let plugin = plugin_directory.join("docker-buildx");
    fs::copy(&inputs.plugin, &plugin)
        .map_err(|error| OrchestratorError::io(plugin.display().to_string(), error.to_string()))?;
    fs::set_permissions(&plugin, fs::Permissions::from_mode(0o755))
        .map_err(|error| OrchestratorError::io(plugin.display().to_string(), error.to_string()))?;
    if support::hash_regular(&plugin, support::MAX_BUILDX_BYTES)? != inputs.lock.sha256 {
        return Err(internal("image_candidate_buildx_copy"));
    }
    Ok(BuildContext {
        archive: directory.path().join("candidate.oci.tar"),
        directory,
        docker_config,
        plugin,
    })
}

pub(super) fn run_build(
    context: &BuildContext,
    lock: &BuildxLock,
    source_commit: &str,
    target: DockerTarget<'_>,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let version = ImageCandidateCommand::buildx_version(&context.docker_config)
        .map_err(|_| internal("image_candidate_buildx_version_command"))?;
    let version = docker_target(version, &target)?;
    let version_text = ImageCandidateCommand::run(&version, deadline)
        .map_err(|_| internal("image_candidate_buildx_version"))?;
    let builder = version_text
        .stdout_text("docker")
        .map_err(|_| internal("image_candidate_buildx_version"))?
        .trim()
        .to_owned();
    if buildx_release_version(&builder) != Some(lock.version.as_str()) {
        return Err(internal("image_candidate_buildx_version_mismatch"));
    }
    verify_plugin_info(
        &context.docker_config,
        &context.plugin,
        &lock.version,
        &target,
        deadline,
    )?;
    let directory = context.archive.parent().ok_or_else(|| internal("image_candidate_context"))?;
    let command = match target {
        DockerTarget::Production => ImageCandidateCommand::build_oci(directory, &context.archive, &context.docker_config, source_commit),
        DockerTarget::Qualification { base_layout, .. } => ImageCandidateCommand::build_qualification_oci(directory, &context.archive, &context.docker_config, source_commit, base_layout),
    }
    .map_err(|_| internal("image_candidate_build_command"))?;
    let command = docker_target(command, &target)?;
    ImageCandidateCommand::run(&command, deadline)
        .map_err(|_| internal("image_candidate_build"))?;
    Ok(builder)
}

pub(super) fn docker_target(
    command: velnor_actions_mise::IsolatedCommand,
    target: &DockerTarget<'_>,
) -> Result<velnor_actions_mise::IsolatedCommand, OrchestratorError> {
    match target {
        DockerTarget::Production => Ok(command),
        DockerTarget::Qualification { container_id, .. } => command
            .in_qualification_container(container_id)
            .map_err(|_| internal("native_qualification_docker_endpoint")),
    }
}

fn build_record(
    request: &BuildJobCandidateRequest,
    inputs: CandidateInputs,
    archive: &Path,
    builder: String,
) -> Result<ImageCandidateRecord, OrchestratorError> {
    let graph =
        velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_oci_candidate(archive)
            .map_err(|_| internal("image_candidate_oci_invalid"))?;
    let record = ImageCandidateRecord {
        schema: velnor_actions_contract::IMAGE_CANDIDATE_SCHEMA.to_owned(),
        source: ImageCandidateSource {
            commit: request.source_commit.clone(),
            source_ref: inputs.source_ref,
            inputs: inputs.source_inputs,
        },
        workflow: inputs.workflow,
        platform: "linux/amd64".to_owned(),
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
    };
    velnor_actions_contract::validate_image_candidate_record(&record)
        .map_err(|_| internal("image_candidate_record_invalid"))?;
    Ok(record)
}

fn publish_candidate(
    context: &BuildContext,
    request: &BuildJobCandidateRequest,
    record: &ImageCandidateRecord,
) -> Result<(), OrchestratorError> {
    let bytes = serialize_image_candidate_record(record)
        .map_err(|_| internal("image_candidate_record_invalid"))?;
    publish_bytes(context, request, &bytes)
}

pub(super) fn publish_bytes(
    context: &BuildContext,
    request: &BuildJobCandidateRequest,
    bytes: &[u8],
) -> Result<(), OrchestratorError> {
    let staged_record = context.directory.path().join("candidate.json");
    write_staged(&staged_record, bytes)?;
    fs::hard_link(&context.archive, &request.archive_path).map_err(|error| {
        OrchestratorError::io(
            request.archive_path.display().to_string(),
            error.to_string(),
        )
    })?;
    if let Err(error) = fs::hard_link(&staged_record, &request.record_path) {
        remove_our_archive_link(&context.archive, &request.archive_path);
        return Err(OrchestratorError::io(
            request.record_path.display().to_string(),
            error.to_string(),
        ));
    }
    Ok(())
}

fn verify_closure(root: &Path, commit: &str, path: &Path) -> Result<(), OrchestratorError> {
    let bytes = read_limited(path, 1024 * 1024)?;
    let stored = parse_image_candidate_source_closure(&bytes)
        .map_err(|_| internal("image_candidate_closure_invalid"))?;
    if stored != source::source_closure(root, commit)? {
        return Err(internal("image_candidate_closure_mismatch"));
    }
    Ok(())
}

fn verify_plugin_info(
    config: &Path,
    plugin: &Path,
    version: &str,
    target: &DockerTarget<'_>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let command = ImageCandidateCommand::buildx_plugin_info(config)
        .map_err(|_| internal("image_candidate_plugin_info_command"))?;
    let command = docker_target(command, target)?;
    let output = ImageCandidateCommand::run(&command, deadline)
        .map_err(|_| internal("image_candidate_plugin_info"))?;
    let text = output
        .stdout_text("docker")
        .map_err(|_| internal("image_candidate_plugin_info"))?;
    let value = velnor_actions_contract::parse_strict_json(&text)
        .map_err(|_| internal("image_candidate_plugin_json"))?;
    let plugins = value
        .as_array()
        .ok_or_else(|| internal("image_candidate_plugin_shape"))?;
    let found = plugins
        .iter()
        .filter(|entry| entry.get("Name").and_then(serde_json::Value::as_str) == Some("buildx"))
        .collect::<Vec<_>>();
    let expected_version = format!("v{version}");
    if found.len() != 1
        || found[0].get("Path").and_then(serde_json::Value::as_str) != plugin.to_str()
        || found[0].get("Version").and_then(serde_json::Value::as_str)
            != Some(expected_version.as_str())
    {
        return Err(internal("image_candidate_plugin_identity"));
    }
    Ok(())
}

fn validate_outputs(
    root: &Path,
    archive: &Path,
    record: &Path,
) -> Result<PathBuf, OrchestratorError> {
    if !archive.is_absolute()
        || !record.is_absolute()
        || archive == record
        || archive.parent() != record.parent()
        || archive.starts_with(root)
        || record.starts_with(root)
        || archive.to_str().is_none_or(|path| path.contains(','))
    {
        return Err(internal("image_candidate_output_paths"));
    }
    let parent = archive
        .parent()
        .ok_or_else(|| internal("image_candidate_output_parent"))?;
    let metadata =
        fs::symlink_metadata(parent).map_err(|_| internal("image_candidate_output_parent"))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != crate::filesystem_owner::effective_uid()
        || support::path_exists(archive)?
        || support::path_exists(record)?
    {
        return Err(internal("image_candidate_output_exclusive"));
    }
    Ok(parent.to_path_buf())
}
