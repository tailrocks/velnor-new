use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
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
use super::support;

struct CandidateInputs {
    root: PathBuf,
    output_parent: PathBuf,
    plugin: PathBuf,
    source_inputs: BTreeMap<String, String>,
    source_ref: String,
    workflow: velnor_actions_contract::ImageCandidateWorkflow,
    lock: BuildxLock,
}

struct BuildContext {
    directory: TempDir,
    docker_config: PathBuf,
    plugin: PathBuf,
    archive: PathBuf,
}

pub(super) fn build_job_candidate(
    request: &BuildJobCandidateRequest,
) -> Result<(), OrchestratorError> {
    request
        .deadline
        .remaining()
        .map_err(|_| internal("image_candidate_deadline"))?;
    let inputs = prepare_inputs(request)?;
    let context = prepare_context(&inputs)?;
    let builder = run_build(&context, &inputs.lock, request.deadline)?;
    let record = build_record(request, inputs, &context.archive, builder)?;
    publish_candidate(&context, request, &record)
}

fn prepare_inputs(
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

fn prepare_context(inputs: &CandidateInputs) -> Result<BuildContext, OrchestratorError> {
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
    let dockerfile = inputs.root.join("images/job/ubuntu-26.04/Dockerfile");
    let expected = source::hash_source_file(&inputs.root, "images/job/ubuntu-26.04/Dockerfile")?;
    copy_source_file(&dockerfile, &directory.path().join("Dockerfile"), &expected)?;
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

fn run_build(
    context: &BuildContext,
    lock: &BuildxLock,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let version = ImageCandidateCommand::buildx_version(&context.docker_config)
        .map_err(|_| internal("image_candidate_buildx_version_command"))?;
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
        deadline,
    )?;
    let command = ImageCandidateCommand::build_oci(
        context
            .archive
            .parent()
            .ok_or_else(|| internal("image_candidate_context"))?,
        &context.archive,
        &context.docker_config,
    )
    .map_err(|_| internal("image_candidate_build_command"))?;
    ImageCandidateCommand::run(&command, deadline)
        .map_err(|_| internal("image_candidate_build"))?;
    Ok(builder)
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
    let staged_record = context.directory.path().join("candidate.json");
    write_staged(&staged_record, &bytes)?;
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
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let command = ImageCandidateCommand::buildx_plugin_info(config)
        .map_err(|_| internal("image_candidate_plugin_info_command"))?;
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

fn copy_source_file(
    source_path: &Path,
    destination: &Path,
    expected_sha: &str,
) -> Result<(), OrchestratorError> {
    let hash_from = |path: &Path| {
        source::hash_source_file(
            path.parent()
                .ok_or_else(|| internal("image_candidate_copy_path"))?,
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .ok_or_else(|| internal("image_candidate_copy_path"))?,
        )
    };
    if hash_from(source_path)? != expected_sha {
        return Err(internal("image_candidate_copy_source_changed"));
    }
    fs::copy(source_path, destination).map_err(|error| {
        OrchestratorError::io(destination.display().to_string(), error.to_string())
    })?;
    if hash_from(destination)? != expected_sha {
        return Err(internal("image_candidate_copy_changed"));
    }
    Ok(())
}

fn write_staged(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))
}

fn remove_our_archive_link(stage: &Path, output: &Path) {
    let stage_metadata = fs::metadata(stage).ok();
    let output_metadata = fs::symlink_metadata(output).ok();
    if stage_metadata
        .zip(output_metadata)
        .is_some_and(|(left, right)| left.dev() == right.dev() && left.ino() == right.ino())
    {
        drop(fs::remove_file(output));
    }
}
