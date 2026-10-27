// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::workflow::{
    ExecutionBranch, JobBindingV1, TaskExecutionManifestV1, UnitLedgerV1, UnitResultV1,
};
use velnor_actions_contract::{
    Plan, parse_strict_json, plan_json_bytes, validate_matrix_key,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

pub const EMIT_GROUP_LEDGER_OP: &str = "emit-group-ledger-v1";

pub(super) const GROUP_LEDGER_FILENAME: &str = "group-ledger.json";

const MAX_PLAN_JSON_BYTES: u64 = 4 << 20;
const MAX_MANIFEST_BYTES: u64 = 4 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobStatus {
    Success,
    Failure,
    Cancelled,
}

#[derive(Debug, Clone)]
pub(crate) struct EmitEnv {
    pub(crate) job_key: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u64,
    pub(crate) event: String,
    pub(crate) source_sha: String,
    pub(crate) ref_name: String,
    pub(crate) repository: String,
    pub(crate) workflow_sha: String,
    pub(crate) matrix_key: Option<String>,
    pub(crate) job_status: JobStatus,
}

/// Read the tokenless emit environment and write the matrix or crate job ledger.
///
/// Identity binds on the repository slug, `run_id`, `run_attempt`, and
/// `job_key`: the numeric `repository_id`/`job_id` are unresolvable without
/// a token, so emit records them as `0` (see `JobBindingV1`).
pub fn emit_group_ledger() -> Result<PathBuf, OrchestratorError> {
    let emit = emit_env_from_process()?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    let runner_temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let run_dir = Path::new(&runner_temp).join("velnor").join(&run_key);
    let checkout = env::current_dir().map_err(|err| OrchestratorError::io(".", err.to_string()))?;
    let manifest_path = checkout.join(velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH);
    emit_group_ledger_to(&runner_temp, &manifest_path, &run_dir, &run_key, &emit)
}

fn emit_env_from_process() -> Result<EmitEnv, OrchestratorError> {
    let job_status = match required_env("VELNOR_JOB_STATUS")?.as_str() {
        "success" => JobStatus::Success,
        "failure" => JobStatus::Failure,
        "cancelled" => JobStatus::Cancelled,
        _ => return Err(internal("bad_job_status")),
    };
    let run_id = required_env("GITHUB_RUN_ID")?
        .parse::<u64>()
        .map_err(|_| internal("bad_run_id"))?;
    if run_id == 0 {
        return Err(internal("bad_run_id"));
    }
    let run_attempt = required_env("GITHUB_RUN_ATTEMPT")?
        .parse::<u64>()
        .map_err(|_| internal("bad_run_attempt"))?;
    if run_attempt == 0 {
        return Err(internal("bad_run_attempt"));
    }
    let repository = required_env("GITHUB_REPOSITORY")?;
    if crate::origin::validate_repository_slug(&repository).is_none() {
        return Err(internal("bad_repository_slug"));
    }
    Ok(EmitEnv {
        job_key: required_env("GITHUB_JOB")?,
        run_id,
        run_attempt,
        event: required_env("GITHUB_EVENT_NAME")?,
        source_sha: required_env("GITHUB_SHA")?,
        ref_name: required_env("GITHUB_REF")?,
        repository,
        workflow_sha: required_env("GITHUB_WORKFLOW_SHA")?,
        matrix_key: match env::var("VELNOR_MATRIX_KEY") {
            Ok(key) => Some(key),
            Err(env::VarError::NotPresent) => None,
            Err(env::VarError::NotUnicode(_)) => return Err(internal("bad_velnor_matrix_key")),
        },
        job_status,
    })
}

fn required_env(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            internal(&format!(
                "missing_{}",
                name.to_ascii_lowercase().replace('-', "_")
            ))
        })
}

pub(crate) fn emit_group_ledger_to(
    anchor: &Path,
    manifest_path: &Path,
    run_dir: &Path,
    run_key: &str,
    emit: &EmitEnv,
) -> Result<PathBuf, OrchestratorError> {
    if let Some(key) = &emit.matrix_key {
        validate_matrix_key(key).map_err(internal_contract)?;
    }
    let plan_text =
        crate::retrieve_reports::read_staged_text(&run_dir.join("plan.json"), MAX_PLAN_JSON_BYTES)
            .map_err(|kind| internal(&format!("{kind}_plan")))?;
    let plan_value = parse_strict_json(&plan_text).map_err(|_| internal("unparsable_plan"))?;
    let plan: Plan = serde_json::from_value(plan_value).map_err(|_| internal("unparsable_plan"))?;
    plan.validate().map_err(internal_contract)?;
    if plan.run_key != run_key {
        return Err(internal("ledger_run_mismatch"));
    }
    let manifest_text =
        crate::retrieve_reports::read_staged_text(manifest_path, MAX_MANIFEST_BYTES)
            .map_err(|kind| internal(&format!("{kind}_manifest")))?;
    let manifest = parse_manifest_document(&manifest_text)?;
    let (group_key, branch) = split_job_key(&emit.job_key)?;
    let plan_canonical = plan_json_bytes(&plan).map_err(internal_contract)?;
    let plan_digest = UnitLedgerV1::plan_digest_from_canonical_bytes(&plan_canonical);
    let group_digest =
        UnitLedgerV1::group_digest_for_group(group_key, &plan_digest).map_err(internal_contract)?;
    let finished_secs = clock_secs()?;
    let (ordered_results, observation_sidecars) =
        selection::build_ordered_results(&plan, &manifest, run_key, emit, run_dir, finished_secs)?;
    let observation_refs: Vec<Option<&[u8]>> = observation_sidecars
        .iter()
        .map(|sidecar| sidecar.as_deref())
        .collect();
    let observation_digest = UnitLedgerV1::observation_digest_for_sidecars(&observation_refs)
        .map_err(internal_contract)?;
    let source_binding_digest =
        UnitLedgerV1::source_binding_digest_for_sidecars(&vec![None; ordered_results.len()])
            .map_err(internal_contract)?;
    let ledger = UnitLedgerV1 {
        schema: 1,
        job: JobBindingV1 {
            schema: 1,
            repository_id: 0,
            repository: emit.repository.clone(),
            run_id: emit.run_id,
            run_attempt: emit.run_attempt,
            job_id: 0,
            job_key: emit.job_key.clone(),
            event: emit.event.clone(),
            ref_name: emit.ref_name.clone(),
            workflow_sha: emit.workflow_sha.clone(),
            source_sha: emit.source_sha.clone(),
            plan_digest,
        },
        group_digest,
        execution_branch: branch,
        selected_unit_ids: ordered_results
            .iter()
            .map(|result| result.unit_id.clone())
            .collect(),
        ordered_results,
        observation_digest,
        source_binding_digest,
    };
    ledger.validate().map_err(internal_contract)?;
    let bytes = ledger.to_canonical_bytes().map_err(internal_contract)?;
    let out_dir = emit
        .matrix_key
        .as_ref()
        .map_or_else(|| run_dir.to_path_buf(), |key| run_dir.join(key));
    crate::exclusive_write::create_dir_no_symlink(anchor, &out_dir)?;
    let out_path = out_dir.join(GROUP_LEDGER_FILENAME);
    crate::exclusive_write::write_exclusive(&out_path, &bytes, "group_ledger")?;
    Ok(out_path)
}

type OrderedUnits = (Vec<UnitResultV1>, Vec<Option<Vec<u8>>>);

#[path = "emit_selection.rs"]
mod selection;

/// Parse the on-disk task execution manifest: the checked-in document
/// carries a generated-marker header line (see `marked_json`) which is
/// stripped before strict parsing; a bare canonical document parses as-is.
fn parse_manifest_document(text: &str) -> Result<TaskExecutionManifestV1, OrchestratorError> {
    let body = match text.split_once('\n') {
        Some((first, rest)) if first.starts_with(velnor_actions_contract::MARKER_PREFIX) => {
            rest.strip_suffix('\n').unwrap_or(rest)
        }
        _ => text,
    };
    if body.bytes().any(|byte| matches!(byte, b'\n' | b'\r')) {
        return Err(internal("noncanonical_task_execution_manifest"));
    }
    let value = parse_strict_json(body).map_err(|_| internal("unparsable_manifest"))?;
    let manifest: TaskExecutionManifestV1 =
        serde_json::from_value(value).map_err(|_| internal("unparsable_manifest"))?;
    manifest.validate().map_err(internal_contract)?;
    Ok(manifest)
}

fn obligation_digest<'a>(plan: &'a Plan, unit_id: &str) -> Result<&'a str, OrchestratorError> {
    plan.obligations
        .iter()
        .find(|obligation| obligation.task_id == unit_id)
        .map(|obligation| obligation.task_digest.as_str())
        .ok_or_else(|| internal(&format!("task_without_obligation:{unit_id}")))
}

fn split_job_key(job_key: &str) -> Result<(&str, ExecutionBranch), OrchestratorError> {
    if let Some(group) =
        job_key.strip_suffix(velnor_actions_contract::workflow::crate_job::GROUP_READER_SUFFIX)
    {
        if group.is_empty() {
            return Err(internal("job_key_without_group"));
        }
        return Ok((group, ExecutionBranch::Reader));
    }
    if let Some(group) =
        job_key.strip_suffix(velnor_actions_contract::workflow::crate_job::GROUP_WRITER_SUFFIX)
    {
        if group.is_empty() {
            return Err(internal("job_key_without_group"));
        }
        return Ok((group, ExecutionBranch::Writer));
    }
    Err(internal("job_key_without_branch_suffix"))
}

fn clock_secs() -> Result<u64, OrchestratorError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| internal("missing_clock"))
}

#[cfg(test)]
#[path = "emit_gc1_t54.rs"]
mod gc1_t54;
