use std::{fs, io::Read, os::unix::fs::MetadataExt, path::Path};
use velnor_actions_contract::{
    ImageCandidateRecord, parse_image_candidate_record, parse_image_candidate_source_closure,
};

use crate::{OrchestratorError, internal::internal};

use super::{ValidateJobCandidateRequest, source};

pub(super) fn validate_job_candidate(
    request: &ValidateJobCandidateRequest,
) -> Result<(), OrchestratorError> {
    request
        .deadline
        .remaining()
        .map_err(|_| internal("image_candidate_deadline"))?;
    let root = source::checked_root(&request.root)?;
    source::ensure_clean_checkout(&root, request.deadline)?;
    let closure = read_limited(&request.source_closure_path, 1024 * 1024)?;
    let closure = parse_image_candidate_source_closure(&closure)
        .map_err(|_| internal("image_candidate_closure_invalid"))?;
    let head = source::git_text(
        &root,
        &velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()]),
        request.deadline,
    )?;
    if closure.source_commit != head.trim()
        || closure != source::source_closure(&root, head.trim())?
    {
        return Err(internal("image_candidate_closure_mismatch"));
    }
    let bytes = read_limited(&request.record_path, 1024 * 1024)?;
    let record = parse_image_candidate_record(&bytes)
        .map_err(|_| internal("image_candidate_record_invalid"))?;
    verify_source_and_workflow(&record, &root, request)?;
    let archive_metadata = fs::symlink_metadata(&request.archive_path).map_err(|error| {
        OrchestratorError::io(
            request.archive_path.display().to_string(),
            error.to_string(),
        )
    })?;
    if !archive_metadata.is_file()
        || archive_metadata.file_type().is_symlink()
        || archive_metadata.len() > 4 * 1024 * 1024 * 1024
    {
        return Err(internal("image_candidate_archive_file"));
    }
    let graph = velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_oci_candidate(
        &request.archive_path,
    )
    .map_err(|_| internal("image_candidate_oci_invalid"))?;
    if record.archive.size_bytes != graph.archive_size_bytes
        || record.archive.sha256 != graph.archive_sha256
        || record.oci.archive_index_digest != graph.archive_index_digest
        || record.oci.archive_index_size_bytes != graph.archive_index_size_bytes
        || record.oci.platform_manifest_digest != graph.platform_manifest_digest
        || record.oci.platform_manifest_size_bytes != graph.platform_manifest_size_bytes
        || record.oci.image_configuration_digest != graph.image_configuration_digest
        || record.oci.image_configuration_size_bytes != graph.image_configuration_size_bytes
        || record.oci.layer_digests != graph.layer_digests
    {
        return Err(internal("image_candidate_oci_identity"));
    }
    Ok(())
}

fn verify_source_and_workflow(
    record: &ImageCandidateRecord,
    root: &Path,
    request: &ValidateJobCandidateRequest,
) -> Result<(), OrchestratorError> {
    let commit = source::git_text(
        root,
        &velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()]),
        request.deadline,
    )?;
    if record.source.commit != commit.trim() || record.source.commit != source::env("GITHUB_SHA")? {
        return Err(internal("image_candidate_source_commit"));
    }
    if record.source.inputs != source::source_inputs(root)? {
        return Err(internal("image_candidate_source_inputs"));
    }
    let workflow = workflow_identity()?;
    if record.workflow != workflow.0 || record.source.source_ref != workflow.1 {
        return Err(internal("image_candidate_workflow_identity"));
    }
    let lock = read_lock(root)?;
    if buildx_release_version(&record.build.builder) != Some(lock.version.as_str()) {
        return Err(internal("image_candidate_buildx_version"));
    }
    Ok(())
}

pub(super) fn workflow_identity()
-> Result<(velnor_actions_contract::ImageCandidateWorkflow, String), OrchestratorError> {
    let repository = source::env("GITHUB_REPOSITORY")?;
    let repository_id = source::env("GITHUB_REPOSITORY_ID")?
        .parse()
        .map_err(|_| internal("image_candidate_repository_id"))?;
    let event = source::env("GITHUB_EVENT_NAME")?;
    let workflow_ref = source::env("GITHUB_WORKFLOW_REF")?;
    let authority_commit = source::env("GITHUB_WORKFLOW_SHA")?;
    let run_id = source::env("GITHUB_RUN_ID")?
        .parse()
        .map_err(|_| internal("image_candidate_run_id"))?;
    let run_attempt = source::env("GITHUB_RUN_ATTEMPT")?
        .parse()
        .map_err(|_| internal("image_candidate_run_attempt"))?;
    let source_ref = source::env("GITHUB_REF")?;
    let workflow = velnor_actions_contract::ImageCandidateWorkflow {
        repository,
        repository_id,
        event,
        workflow_ref,
        authority_commit,
        run_id,
        run_attempt,
    };
    Ok((workflow, source_ref))
}

pub(super) fn buildx_release_version(builder: &str) -> Option<&str> {
    let version = builder.strip_prefix("github.com/docker/buildx v")?;
    let (version, suffix) = version
        .split_once(' ')
        .map_or((version, None), |(version, suffix)| (version, Some(suffix)));
    if suffix.is_some_and(|value| {
        value.is_empty() || value.bytes().any(|byte| matches!(byte, b'\n' | b'\r' | 0))
    }) {
        return None;
    }
    let mut components = version.split('.');
    if !(0..3).all(|_| {
        components
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) || components.next().is_some()
    {
        return None;
    }
    Some(version)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) struct BuildxLock {
    pub(super) version: String,
    pub(super) url: String,
    pub(super) sha256: String,
}

pub(super) fn read_lock(root: &Path) -> Result<BuildxLock, OrchestratorError> {
    let bytes = read_limited(&root.join(".velnor/runner.lock"), 1024 * 1024)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| internal("image_candidate_lock_utf8"))?;
    let value: toml::Value =
        toml::from_str(text).map_err(|_| internal("image_candidate_lock_parse"))?;
    let get = |key: &str| {
        value
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| internal("image_candidate_lock_pin"))
    };
    let lock = BuildxLock {
        version: get("buildx_version")?,
        url: get("buildx_asset_url")?,
        sha256: get("buildx_sha256")?,
    };
    if lock.version.is_empty()
        || !lock
            .url
            .starts_with("https://github.com/docker/buildx/releases/download/")
        || !is_sha256(&lock.sha256)
    {
        return Err(internal("image_candidate_buildx_lock"));
    }
    Ok(lock)
}

pub(super) fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, OrchestratorError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err(internal("image_candidate_file_limit"));
    }
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("image_candidate_file_open"))?;
    let mut file = fs::File::from(fd);
    let opened = file
        .metadata()
        .map_err(|_| internal("image_candidate_file_stat"))?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(internal("image_candidate_file_identity"));
    }
    let capacity =
        usize::try_from(metadata.len()).map_err(|_| internal("image_candidate_file_size"))?;
    let mut bytes = Vec::with_capacity(capacity);
    (&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| internal("image_candidate_file_read"))?;
    let after = file
        .metadata()
        .map_err(|_| internal("image_candidate_file_stat"))?;
    let path_after =
        fs::symlink_metadata(path).map_err(|_| internal("image_candidate_file_stat"))?;
    if bytes.len() as u64 > limit
        || after.len() != metadata.len()
        || after.dev() != metadata.dev()
        || after.ino() != metadata.ino()
        || after.mtime() != metadata.mtime()
        || after.mtime_nsec() != metadata.mtime_nsec()
        || after.ctime() != metadata.ctime()
        || after.ctime_nsec() != metadata.ctime_nsec()
        || path_after.dev() != metadata.dev()
        || path_after.ino() != metadata.ino()
        || !path_after.is_file()
        || path_after.file_type().is_symlink()
    {
        return Err(internal("image_candidate_file_changed"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{buildx_release_version, read_lock};
    use std::path::Path;

    #[test]
    fn pinned_lock_projection_reads_only_buildx_values() {
        let lock = read_lock(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .as_path(),
        )
        .expect("workspace lock");
        assert_eq!(lock.version, "0.38.0");
        assert!(lock.url.ends_with("buildx-v0.38.0.linux-amd64"));
        assert_eq!(lock.sha256.len(), 64);
    }

    #[test]
    fn buildx_record_must_match_the_exact_locked_version() {
        let lock = read_lock(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .as_path(),
        )
        .expect("workspace lock");
        let same = format!("github.com/docker/buildx v{} (fixture)", lock.version);
        let prefix_collision = "github.com/docker/buildx v0.38.01 (fixture)";
        assert_eq!(buildx_release_version(&same), Some(lock.version.as_str()));
        assert_ne!(
            buildx_release_version(prefix_collision),
            Some(lock.version.as_str())
        );
        assert_eq!(
            buildx_release_version("github.com/docker/buildx v0.38.0 "),
            None
        );
        assert_eq!(
            buildx_release_version("github.com/docker/buildx v0.38.0\nextra"),
            None
        );
    }
}
