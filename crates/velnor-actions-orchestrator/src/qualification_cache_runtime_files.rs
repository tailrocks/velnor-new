//! Bounded file and context readers for cache binding.

use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::workflow::{
    QualificationCacheAdmission, QualificationRuntimeIdentity, QualificationRuntimeImageDigests,
    QualificationRuntimeImageIdentity, QualificationRuntimeScaleSet, Trust, WorkflowEvent,
};
use velnor_actions_contract::{RunsOn, canonical_json_str, run_key_for_ci};

use crate::OrchestratorError;
use crate::internal::internal;

use super::QualificationCacheBindingRequest;

#[path = "qualification_cache_runtime_files/anchored.rs"]
mod anchored;
pub(super) use anchored::RunDirectory;

pub(super) const MAX_PLAN_JSON_BYTES: u64 = crate::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES;
const GUEST_IMAGE_PROJECTION_PATH: &str = "/.velnor-runtime-identity/guest-image.json";
const GUEST_IMAGE_RUNTIME_DIRECTORY: &str = "/.velnor-runtime-identity";
pub(super) const GUEST_IMAGE_PROJECTION_SCHEMA: &str = "velnor.guest-image/v1";
const MAX_GUEST_IMAGE_PROJECTION_BYTES: usize = 65_536;

pub(super) fn read_plan(
    run_dir: &RunDirectory,
    expected_run_key: &str,
) -> Result<velnor_actions_contract::Plan, OrchestratorError> {
    let plan_limit = usize::try_from(MAX_PLAN_JSON_BYTES)
        .map_err(|_| internal("qualification_cache_plan_limit"))?;
    let plan_bytes =
        run_dir.read_owned_file("plan.json", plan_limit, None, "qualification_cache_file")?;
    let plan_value = parse_strict_json_bytes(&plan_bytes, plan_limit)
        .map_err(|_| internal("qualification_cache_plan_json"))?;
    let plan: velnor_actions_contract::Plan = serde_json::from_value(plan_value)
        .map_err(|_| internal("qualification_cache_plan_shape"))?;
    plan.validate()
        .map_err(crate::internal::internal_contract)?;
    if plan.run_key != expected_run_key {
        return Err(internal("qualification_cache_run_key_mismatch"));
    }
    Ok(plan)
}

pub(super) fn validate_dispatch_context(
    plan: &velnor_actions_contract::Plan,
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<(), OrchestratorError> {
    let context = plan
        .qualification
        .as_ref()
        .ok_or_else(|| internal("qualification_cache_dispatch_missing"))?;
    if plan.event != WorkflowEvent::Qualification
        || plan.trust != Trust::Pr
        || request.event_name != "workflow_dispatch"
        || request.run_id == 0
        || request.run_attempt == 0
        || context.run_id != request.run_id
        || context.run_attempt != request.run_attempt
        || context.source_sha != request.source_sha
        || plan.head != request.source_sha
        || context.workflow_sha != request.workflow_sha
        || context.workflow_ref != request.workflow_ref
        || context.git_ref != request.git_ref
        || context.repository != request.repository
        || context.ref_protected != request.ref_protected
        || run_key_for_ci(context.run_id, u64::from(context.run_attempt)) != plan.run_key
    {
        return Err(internal("qualification_cache_dispatch_mismatch"));
    }
    Ok(())
}

pub(super) fn read_admission(
    run_dir: &RunDirectory,
    plan: &velnor_actions_contract::Plan,
) -> Result<Option<QualificationCacheAdmission>, OrchestratorError> {
    let predecessor_required = plan
        .qualification
        .as_ref()
        .is_some_and(|context| context.phase.predecessor().is_some());
    if !predecessor_required {
        return if run_dir
            .file_exists(crate::qualification_resolver::QUALIFICATION_ADMISSION_FILENAME)?
        {
            Err(internal("unexpected_qualification_cache_admission"))
        } else {
            Ok(None)
        };
    }
    let bytes = run_dir.read_owned_file(
        crate::qualification_resolver::QUALIFICATION_ADMISSION_FILENAME,
        velnor_actions_contract::MAX_QUALIFICATION_RECEIPT_BYTES,
        None,
        "qualification_cache_file",
    )?;
    let _ = parse_strict_json_bytes(
        &bytes,
        velnor_actions_contract::MAX_QUALIFICATION_RECEIPT_BYTES,
    )
    .map_err(|_| internal("qualification_cache_admission_json"))?;
    let admission = QualificationCacheAdmission::parse_bounded(&bytes)
        .map_err(crate::internal::internal_contract)?;
    admission
        .validate_for_plan(plan)
        .map_err(crate::internal::internal_contract)?;
    Ok(Some(admission))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuestImageProjection {
    pub(super) schema: String,
    pub(super) source_sha: String,
    pub(super) guest_platform: String,
    pub(super) scale_set: QualificationRuntimeScaleSet,
    pub(super) images: GuestImageDigests,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuestImageDigests {
    pub(super) runner: QualificationRuntimeImageDigests,
    pub(super) dind: QualificationRuntimeImageDigests,
}

pub(super) fn read_guest_image_projection() -> Result<GuestImageProjection, OrchestratorError> {
    let path = Path::new(GUEST_IMAGE_PROJECTION_PATH);
    let directory = Path::new(GUEST_IMAGE_RUNTIME_DIRECTORY);
    let metadata = fs::symlink_metadata(directory)
        .map_err(|_| internal("qualification_guest_image_directory_missing"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != 0
        || metadata.gid() != 0
        || metadata.mode() & 0o7777 != 0o555
    {
        return Err(internal("qualification_guest_image_directory_untrusted"));
    }
    validate_readonly_runtime_mount(GUEST_IMAGE_RUNTIME_DIRECTORY)?;
    let bytes = read_readonly_projection_file(path)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| internal("qualification_guest_image_encoding"))?;
    parse_canonical(text, MAX_GUEST_IMAGE_PROJECTION_BYTES)
}

fn validate_readonly_runtime_mount(target: &str) -> Result<(), OrchestratorError> {
    let mut mountinfo = fs::File::open("/proc/self/mountinfo")
        .map_err(|_| internal("qualification_guest_image_mount_unreadable"))?
        .take(64 * 1024 + 1);
    let mut bytes = Vec::new();
    mountinfo
        .read_to_end(&mut bytes)
        .map_err(|_| internal("qualification_guest_image_mount_unreadable"))?;
    if bytes.len() > 64 * 1024 {
        return Err(internal("qualification_guest_image_mount_oversize"));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| internal("qualification_guest_image_mount_invalid"))?;
    let mut matching = text
        .lines()
        .filter(|line| line.split_whitespace().nth(4) == Some(target));
    let line = matching
        .next()
        .ok_or_else(|| internal("qualification_guest_image_mount_missing"))?;
    if matching.next().is_some() {
        return Err(internal("qualification_guest_image_mount_duplicate"));
    }
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let mount_options = fields
        .get(5)
        .ok_or_else(|| internal("qualification_guest_image_mount_invalid"))?
        .split(',')
        .collect::<Vec<_>>();
    if !mount_options.contains(&"ro") || mount_options.contains(&"rw") {
        return Err(internal("qualification_guest_image_mount_not_read_only"));
    }
    if text.lines().any(|candidate| {
        candidate
            .split_whitespace()
            .nth(4)
            .is_some_and(|mountpoint| mountpoint.starts_with(&format!("{target}/")))
    }) {
        return Err(internal("qualification_guest_image_mount_shadowed"));
    }
    Ok(())
}

fn read_readonly_projection_file(path: &Path) -> Result<Vec<u8>, OrchestratorError> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("qualification_guest_image_unreadable"))?;
    let stat =
        rustix::fs::fstat(&fd).map_err(|_| internal("qualification_guest_image_unreadable"))?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
        || stat.st_uid != 0
        || stat.st_gid != 0
        || stat.st_mode & 0o222 != 0
        || stat.st_mode & 0o7777 != 0o444
    {
        return Err(internal("qualification_guest_image_file_untrusted"));
    }
    let mut file = fs::File::from(fd).take(
        u64::try_from(MAX_GUEST_IMAGE_PROJECTION_BYTES)
            .map_err(|_| internal("qualification_guest_image_limit"))?
            .saturating_add(1),
    );
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| internal("qualification_guest_image_unreadable"))?;
    if bytes.len() > MAX_GUEST_IMAGE_PROJECTION_BYTES {
        return Err(internal("qualification_guest_image_oversize"));
    }
    Ok(bytes)
}

pub(super) fn verified_scale_set_image_identity(
    projection: &GuestImageProjection,
    source_sha: &str,
    selector: &velnor_actions_contract::ScaleSetSelector,
) -> Result<QualificationRuntimeImageIdentity, OrchestratorError> {
    if projection.schema != GUEST_IMAGE_PROJECTION_SCHEMA
        || projection.source_sha != source_sha
        || projection.guest_platform != "linux/amd64"
        || projection.scale_set.name != selector.name()
        || projection.scale_set.labels != selector.labels()
    {
        return Err(internal("qualification_guest_image_binding_mismatch"));
    }
    Ok(QualificationRuntimeImageIdentity::ScaleSet {
        source_sha: projection.source_sha.clone(),
        guest_platform: projection.guest_platform.clone(),
        scale_set: Box::new(projection.scale_set.clone()),
        runner_image: Box::new(projection.images.runner.clone()),
        dind_image: Box::new(projection.images.dind.clone()),
    })
}

pub(super) fn validate_runtime_environment(
    evidence: &QualificationRuntimeIdentity,
    plan: &velnor_actions_contract::Plan,
    entry: &velnor_actions_contract::workflow::MatrixEntry,
    request: &QualificationCacheBindingRequest<'_>,
    scale_set_image: Option<&QualificationRuntimeImageIdentity>,
) -> Result<(), OrchestratorError> {
    if evidence.platform.os != request.runner_os
        || evidence.platform.arch != request.runner_arch
        || evidence.platform.runner_label != plan.runner.label
        || evidence.platform.runs_on != entry.planned_platform.runs_on
        || evidence.platform.target != entry.planned_platform.target
    {
        return Err(internal("qualification_cache_runtime_environment_mismatch"));
    }
    match RunsOn::parse(&entry.planned_platform.runs_on)
        .map_err(crate::internal::internal_contract)?
    {
        RunsOn::Hosted(_) => {
            let QualificationRuntimeImageIdentity::Hosted { image_os } =
                &evidence.platform.image_identity
            else {
                return Err(internal(
                    "qualification_cache_runtime_image_source_mismatch",
                ));
            };
            if request.image_os != Some(image_os.as_str()) {
                return Err(internal("qualification_cache_runtime_image_mismatch"));
            }
        }
        RunsOn::ScaleSet(_) => {
            if request.image_os.is_some()
                || scale_set_image != Some(&evidence.platform.image_identity)
            {
                return Err(internal("qualification_cache_runtime_image_mismatch"));
            }
        }
    }
    Ok(())
}

pub(super) fn parse_canonical<T>(text: &str, max_bytes: usize) -> Result<T, OrchestratorError>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    let bytes = text.as_bytes();
    let value = parse_strict_json_bytes(bytes, max_bytes)
        .map_err(|_| internal("qualification_cache_input_json"))?;
    let parsed: T =
        serde_json::from_value(value).map_err(|_| internal("qualification_cache_input_shape"))?;
    if canonical_json_str(&parsed).map_err(crate::internal::internal_contract)? != text {
        return Err(internal("qualification_cache_input_not_canonical"));
    }
    Ok(parsed)
}

pub(super) fn parse_layer(
    value: &str,
) -> Result<velnor_actions_contract::workflow::QualificationCacheLayer, OrchestratorError> {
    use velnor_actions_contract::workflow::QualificationCacheLayer;

    match value {
        "mbx_objects" => Ok(QualificationCacheLayer::MbxObjects),
        "cargo_sources" => Ok(QualificationCacheLayer::CargoSources),
        "mise_tools" => Ok(QualificationCacheLayer::MiseTools),
        "tofu_providers" => Ok(QualificationCacheLayer::TofuProviders),
        "task_result" => Ok(QualificationCacheLayer::TaskResult),
        _ => Err(internal("qualification_cache_layer_unknown")),
    }
}

pub(super) fn validate_runner_temp(path: &Path) -> Result<(), OrchestratorError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(internal("qualification_cache_runner_temp_path"));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| internal("qualification_cache_runner_temp_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || !owned_by_current_user(&metadata)
    {
        return Err(internal("qualification_cache_runner_temp_untrusted"));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn read_owned_runner_file(
    path: &Path,
    limit: usize,
) -> Result<Vec<u8>, OrchestratorError> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("qualification_cache_file_unreadable"))?;
    let stat =
        rustix::fs::fstat(&fd).map_err(|_| internal("qualification_cache_file_unreadable"))?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
        || stat.st_uid != crate::filesystem_owner::effective_uid()
    {
        return Err(internal("qualification_cache_file_untrusted"));
    }
    let limit = u64::try_from(limit).map_err(|_| internal("qualification_cache_file_limit"))?;
    let mut file = fs::File::from(fd).take(limit.saturating_add(1));
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| internal("qualification_cache_file_unreadable"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(internal("qualification_cache_file_oversize"));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn owned_by_current_user(metadata: &fs::Metadata) -> bool {
    metadata.uid() == crate::filesystem_owner::effective_uid()
}

#[cfg(not(unix))]
fn owned_by_current_user(_metadata: &fs::Metadata) -> bool {
    false
}
