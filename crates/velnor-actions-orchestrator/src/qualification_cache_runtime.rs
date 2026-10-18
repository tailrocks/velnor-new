//! Bind one generated cache directive to the current runner.

use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
use velnor_actions_contract::workflow::{
    QualificationCacheAdmission, QualificationCacheBackendEntry, QualificationCacheDirective,
    QualificationCacheLayer, QualificationRuntimeIdentity, QualificationRuntimeImageDigests,
    QualificationRuntimeImageIdentity, QualificationRuntimeScaleSet, Trust, WorkflowEvent,
};
use velnor_actions_contract::{RunsOn, canonical_json_str, run_key_for_ci};

use crate::OrchestratorError;
use crate::internal::internal;

/// Private operation tag for binding one validated cache layer.
pub const QUALIFICATION_CACHE_BIND_OP: &str = "bind-qualification-cache-v1";

/// Fixed canonical directive artifact in the admitted run directory.
pub(crate) const QUALIFICATION_CACHE_DIRECTIVE_FILENAME: &str =
    "qualification-cache-directives.json";
/// Maximum directive bytes implied by the UTF-16 job-output budget.
///
/// The largest UTF-8 representation is three bytes per UTF-16 code unit.
pub(crate) const QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES: usize =
    crate::plan_output_limits::JOB_OUTPUTS_BUDGET_UTF16_BYTES * 3 / 2;
/// Maximum JSON passed through the runtime-identity environment value.
///
/// This stays below common per-environment-entry `exec` limits after the
/// variable name and separators are included.
const MAX_RUNTIME_IDENTITY_JSON_BYTES: usize = 65_536;
const MAX_PLAN_JSON_BYTES: u64 = crate::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES;
const GUEST_IMAGE_PROJECTION_PATH: &str = "/opt/velnor/guest-image.json";
const GUEST_IMAGE_PROJECTION_SCHEMA: &str = "velnor.guest-image/v1";
const MAX_GUEST_IMAGE_PROJECTION_BYTES: usize = 65_536;

/// Current runner facts checked against the staged qualification plan.
#[derive(Debug, Clone)]
pub struct QualificationCacheBindingRequest<'a> {
    /// Runner temporary root.
    pub runner_temp: &'a Path,
    /// Actual run identifier.
    pub run_id: u64,
    /// Actual run attempt.
    pub run_attempt: u32,
    /// Actual event name.
    pub event_name: &'a str,
    /// Actual repository slug.
    pub repository: &'a str,
    /// Actual full Git ref.
    pub git_ref: &'a str,
    /// Actual protected-ref flag.
    pub ref_protected: bool,
    /// Actual workflow ref.
    pub workflow_ref: &'a str,
    /// Actual workflow source revision.
    pub workflow_sha: &'a str,
    /// Actual checked-out source revision.
    pub source_sha: &'a str,
    /// Actual runner operating system.
    pub runner_os: &'a str,
    /// Actual runner architecture.
    pub runner_arch: &'a str,
    /// Verified runner image family.
    pub image_os: Option<&'a str>,
    /// Verified runner image version.
    pub image_version: Option<&'a str>,
    /// Selected plan matrix key.
    pub matrix_key: &'a str,
    /// Closed layer wire name.
    pub layer: &'a str,
    /// Canonical runtime identity collected by the consumer job.
    pub runtime_identity_json: &'a str,
}

/// Exact keys and predecessor evidence for one cache layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheBindingOutputs {
    /// Whether the typed directive activates this layer.
    pub active: bool,
    /// Campaign namespace from the validated plan.
    pub campaign: String,
    /// Phase from the validated plan.
    pub phase: String,
    /// Current plan identifier.
    pub plan_id: String,
    /// Current run key.
    pub run_key: String,
    /// Selected matrix key.
    pub matrix_key: String,
    /// Selected layer wire name.
    pub layer: String,
    /// Exact primary restore key, when restore is enabled.
    pub restore_key: Option<String>,
    /// Exact predecessor cache entry, or null when a miss is required.
    pub expected_cache: Option<QualificationCacheBackendEntry>,
    /// Exact save key, when saving is enabled.
    pub save_key: Option<String>,
    /// Whether save is conditional on a changed useful-state digest.
    pub save_if_state_changes: bool,
    /// Admitted predecessor useful-state digest, when present.
    pub expected_prior_state_digest: Option<String>,
    /// Digest of the observed runtime identity used in the keys.
    pub runtime_identity_digest: Option<String>,
}

/// Bind a directive to an exact plan lane and runner identity.
///
/// The serialized directive cannot grant access. The function rebuilds it
/// from the validated plan and admitted predecessor before returning keys.
/// # Errors
pub fn bind_qualification_cache_runtime(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<QualificationCacheBindingOutputs, OrchestratorError> {
    validate_runner_temp(request.runner_temp)?;
    let layer = parse_layer(request.layer)?;
    let expected_run_key = run_key_for_ci(request.run_id, u64::from(request.run_attempt));
    let run_dir = checked_run_dir(request.runner_temp, &expected_run_key)?;
    let plan = read_plan(&run_dir, &expected_run_key)?;
    validate_dispatch_context(&plan, request)?;

    let admission_path =
        run_dir.join(crate::qualification_resolver::QUALIFICATION_ADMISSION_FILENAME);
    let admission = read_admission(&admission_path, &plan)?;
    let directive_path = run_dir.join(QUALIFICATION_CACHE_DIRECTIVE_FILENAME);
    let directive_bytes =
        read_owned_runner_file(&directive_path, QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES)?;
    let directive_text = std::str::from_utf8(&directive_bytes)
        .map_err(|_| internal("qualification_cache_directive_encoding"))?;
    let directive = parse_canonical::<QualificationCacheDirective>(
        directive_text,
        QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES,
    )?;
    let evidence = parse_canonical::<QualificationRuntimeIdentity>(
        request.runtime_identity_json,
        MAX_RUNTIME_IDENTITY_JSON_BYTES,
    )?;
    let entry = plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.matrix_key == request.matrix_key)
        .ok_or_else(|| internal("qualification_cache_matrix_key_missing"))?;
    let image_identity = match RunsOn::parse(&entry.planned_platform.runs_on)
        .map_err(crate::internal::internal_contract)?
    {
        RunsOn::Hosted(_) => None,
        RunsOn::ScaleSet(selector) => {
            let expected_source_sha = plan
                .qualification
                .as_ref()
                .and_then(|context| context.scale_set_image_source_sha.as_deref())
                .ok_or_else(|| internal("qualification_guest_image_source_authority_missing"))?;
            let projection = read_guest_image_projection()?;
            Some(verified_scale_set_image_identity(
                &projection,
                expected_source_sha,
                &selector,
            )?)
        }
    };
    validate_runtime_environment(&evidence, &plan, entry, request, image_identity.as_ref())?;
    let lane = directive
        .lanes
        .iter()
        .find(|lane| lane.matrix_key == request.matrix_key)
        .ok_or_else(|| internal("qualification_cache_lane_missing"))?;
    if lane.stack_id != entry.stack_id || lane.task_id != entry.task_id {
        return Err(internal("qualification_cache_lane_identity_mismatch"));
    }
    let layer_directive = directive
        .layer(&entry.matrix_key, layer)
        .ok_or_else(|| internal("qualification_cache_layer_missing"))?;
    let bound = directive
        .bind_runtime(
            &plan,
            admission.as_ref(),
            &entry.matrix_key,
            layer,
            &evidence,
        )
        .map_err(crate::internal::internal_contract)?;
    if !layer_directive.active && (bound.restore.is_some() || bound.save_key.is_some()) {
        return Err(internal("qualification_cache_disabled_layer_bound"));
    }
    let restore_key = bound
        .restore
        .as_ref()
        .map(|restore| restore.requested_key.clone());
    let expected_cache = bound
        .restore
        .as_ref()
        .and_then(|restore| restore.expected_cache.clone());
    Ok(QualificationCacheBindingOutputs {
        active: layer_directive.active,
        campaign: directive.campaign,
        phase: directive.phase.as_str().to_owned(),
        plan_id: plan.plan_id,
        run_key: plan.run_key,
        matrix_key: request.matrix_key.to_owned(),
        layer: layer.as_str().to_owned(),
        restore_key,
        expected_cache,
        save_key: bound.save_key,
        save_if_state_changes: bound.save_if_state_changes,
        expected_prior_state_digest: bound.expected_prior_state_digest,
        runtime_identity_digest: bound.runtime_identity_digest,
    })
}

fn read_plan(
    run_dir: &Path,
    expected_run_key: &str,
) -> Result<velnor_actions_contract::Plan, OrchestratorError> {
    let plan_path = run_dir.join("plan.json");
    let plan_limit = usize::try_from(MAX_PLAN_JSON_BYTES)
        .map_err(|_| internal("qualification_cache_plan_limit"))?;
    let plan_bytes = read_owned_runner_file(&plan_path, plan_limit)?;
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

fn validate_dispatch_context(
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

fn read_admission(
    path: &Path,
    plan: &velnor_actions_contract::Plan,
) -> Result<Option<QualificationCacheAdmission>, OrchestratorError> {
    let predecessor_required = plan
        .qualification
        .as_ref()
        .is_some_and(|context| context.phase.predecessor().is_some());
    if !predecessor_required {
        return match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(internal("unexpected_qualification_cache_admission")),
        };
    }
    let bytes = read_owned_runner_file(
        path,
        velnor_actions_contract::MAX_QUALIFICATION_RECEIPT_BYTES,
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
struct GuestImageProjection {
    schema: String,
    source_sha: String,
    guest_platform: String,
    scale_set: QualificationRuntimeScaleSet,
    images: GuestImageDigests,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GuestImageDigests {
    runner: QualificationRuntimeImageDigests,
    dind: QualificationRuntimeImageDigests,
}

fn read_guest_image_projection() -> Result<GuestImageProjection, OrchestratorError> {
    let path = Path::new(GUEST_IMAGE_PROJECTION_PATH);
    for directory in [Path::new("/opt"), Path::new("/opt/velnor")] {
        let metadata = fs::symlink_metadata(directory)
            .map_err(|_| internal("qualification_guest_image_directory_missing"))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || metadata.uid() != 0
            || metadata.mode() & 0o022 != 0
        {
            return Err(internal("qualification_guest_image_directory_untrusted"));
        }
    }
    let bytes = read_readonly_projection_file(path)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| internal("qualification_guest_image_encoding"))?;
    parse_canonical(text, MAX_GUEST_IMAGE_PROJECTION_BYTES)
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
        || stat.st_mode & 0o222 != 0
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

fn verified_scale_set_image_identity(
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

fn validate_runtime_environment(
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
            let QualificationRuntimeImageIdentity::Hosted {
                image_os,
                image_version,
            } = &evidence.platform.image_identity
            else {
                return Err(internal(
                    "qualification_cache_runtime_image_source_mismatch",
                ));
            };
            if request.image_os != Some(image_os.as_str())
                || request.image_version != Some(image_version.as_str())
            {
                return Err(internal("qualification_cache_runtime_image_mismatch"));
            }
        }
        RunsOn::ScaleSet(_) => {
            if request.image_os.is_some()
                || request.image_version.is_some()
                || scale_set_image != Some(&evidence.platform.image_identity)
            {
                return Err(internal("qualification_cache_runtime_image_mismatch"));
            }
        }
    }
    Ok(())
}

fn parse_canonical<T>(text: &str, max_bytes: usize) -> Result<T, OrchestratorError>
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

fn parse_layer(value: &str) -> Result<QualificationCacheLayer, OrchestratorError> {
    match value {
        "mbx_objects" => Ok(QualificationCacheLayer::MbxObjects),
        "cargo_sources" => Ok(QualificationCacheLayer::CargoSources),
        "mise_tools" => Ok(QualificationCacheLayer::MiseTools),
        "tofu_providers" => Ok(QualificationCacheLayer::TofuProviders),
        "task_result" => Ok(QualificationCacheLayer::TaskResult),
        _ => Err(internal("qualification_cache_layer_unknown")),
    }
}

fn validate_runner_temp(path: &Path) -> Result<(), OrchestratorError> {
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

fn checked_run_dir(runner_temp: &Path, run_key: &str) -> Result<PathBuf, OrchestratorError> {
    velnor_actions_contract::validate_run_key(run_key)
        .map_err(crate::internal::internal_contract)?;
    let velnor_dir = runner_temp.join("velnor");
    let run_dir = velnor_dir.join(run_key);
    for path in [&velnor_dir, &run_dir] {
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| internal("qualification_cache_plan_directory_missing"))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || !owned_by_current_user(&metadata)
        {
            return Err(internal("qualification_cache_plan_directory_untrusted"));
        }
    }
    Ok(run_dir)
}

fn read_owned_runner_file(path: &Path, limit: usize) -> Result<Vec<u8>, OrchestratorError> {
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
        || stat.st_uid != rustix::process::geteuid().as_raw()
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
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == rustix::process::geteuid().as_raw()
}

#[cfg(not(unix))]
fn owned_by_current_user(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
#[path = "qualification_cache_runtime_tests.rs"]
mod tests;
