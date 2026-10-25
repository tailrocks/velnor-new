use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, ContractError, NotSelectedReason, ObligationDecision,
    PlannedPlatform, PlatformBinding, PlatformRunnerEnvironment, PlatformUnavailableReason,
    RunnerImageEvidence, Stack, TaskReport, TaskStatus, Trust, WorkflowEvent, join_runner_temp,
    task_report_id_for_task, validate_run_key,
};
use velnor_actions_rust::SelectionBroadening;

use crate::OrchestratorError;
use crate::internal::internal_contract;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataFailure {
    Malformed,
    Incomplete,
}

const OFFLINE_MARKERS: [&str; 8] = [
    "offline",
    "network",
    "failed to download",
    "could not resolve",
    "connection",
    "timed out",
    "timeout",
    "temporary failure",
];

#[must_use]
pub fn classify_metadata_failure(stderr: &str) -> MetadataFailure {
    let lower = stderr.to_lowercase();
    if OFFLINE_MARKERS.iter().any(|mark| lower.contains(mark)) {
        MetadataFailure::Incomplete
    } else {
        MetadataFailure::Malformed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHit {
    Execute,
    Verified,
}

#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent decision signals read clearest as named bools"
)]
pub struct ObligationInputs {
    pub schema_known: bool,
    pub undeclared_inputs: bool,
    pub nondeterministic: bool,
    pub inputs_controlled: bool,
    pub cache_hit: CacheHit,
    pub restore_miss_reason: Option<&'static str>,
}

#[must_use]
pub fn classify_obligation(inputs: &ObligationInputs) -> (ObligationDecision, &'static str) {
    if !inputs.schema_known {
        return (ObligationDecision::Execute, "unknown_extension_schema");
    }
    if inputs.undeclared_inputs {
        return (ObligationDecision::Execute, "undeclared_inputs");
    }
    if inputs.nondeterministic && !inputs.inputs_controlled {
        return (ObligationDecision::Execute, "always_run_dynamic_inputs");
    }
    if inputs.cache_hit == CacheHit::Verified {
        return (
            ObligationDecision::ReusedFromTaskCache,
            "reused_from_task_cache",
        );
    }
    if let Some(reason) = inputs.restore_miss_reason {
        return (ObligationDecision::Execute, reason);
    }
    (ObligationDecision::Execute, "selected")
}

#[derive(Debug, Clone)]
pub struct NotSelectedInputs<'a> {
    pub run_key: &'a str,
    pub event: WorkflowEvent,
    pub trust: Trust,
    pub matrix_id: &'a str,
    pub planned_platform: &'a PlannedPlatform,
    pub matrix_key: &'a str,
    pub task_id: &'a str,
    pub task_digest: &'a str,
    pub reason: NotSelectedReason,
}

pub fn not_selected_report(inputs: &NotSelectedInputs<'_>) -> Result<TaskReport, ContractError> {
    let task_report_id =
        task_report_id_for_task(inputs.run_key, inputs.matrix_key, inputs.task_digest)?;
    let report = TaskReport {
        schema: TaskReport::SCHEMA,
        task_report_id,
        run_key: inputs.run_key.to_owned(),
        event: inputs.event,
        trust: inputs.trust,
        matrix_id: inputs.matrix_id.to_owned(),
        matrix_key: inputs.matrix_key.to_owned(),
        task_id: inputs.task_id.to_owned(),
        task_digest: inputs.task_digest.to_owned(),
        status: TaskStatus::NotSelected,
        not_selected_reason: Some(inputs.reason),
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: String::new(),
            result: CacheResult::NotAttempted,
            miss_reason: None,
        },
        platform_binding: PlatformBinding::unavailable(
            &inputs.planned_platform.platform_id,
            PlatformRunnerEnvironment::Unknown,
            PlatformUnavailableReason::ObservationNotRecorded,
        )?,
        exit_code: 0,
        duration_ms: None,
        outputs: Vec::new(),
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    };
    report.validate()?;
    report.validate_outputs_declared(&[])?;
    Ok(report)
}

pub(crate) fn runner_image_evidence() -> RunnerImageEvidence {
    RunnerImageEvidence::unobserved()
}

#[must_use]
pub fn selection_broadens_for_path(path: &str) -> Option<&'static str> {
    if path.starts_with('/') || path.split('/').any(|segment| segment == "..") {
        return Some("outside_project_path:selecting_all");
    }
    if path == ".velnor" || path.starts_with(".velnor/") || path.starts_with(".github/") {
        return Some("global_config_changed:selecting_all");
    }
    None
}

#[must_use]
pub(crate) fn broadening_for_path(path: &str) -> Option<SelectionBroadening> {
    for stack in Stack::all() {
        match stack {
            Stack::Rust => {
                if let Some(class) = velnor_actions_rust::selection_broadening(path) {
                    return Some(class);
                }
            }
            Stack::Tofu | Stack::Mise | Stack::Native => {}
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskOmission {
    pub task_id: String,
    pub reason: &'static str,
}

#[must_use]
pub fn omission_ledger(
    all_task_ids: &[String],
    selected_task_ids: &BTreeSet<String>,
) -> Vec<TaskOmission> {
    let mut omitted: Vec<TaskOmission> = all_task_ids
        .iter()
        .filter(|id| !selected_task_ids.contains(*id))
        .map(|id| TaskOmission {
            task_id: id.clone(),
            reason: "not_affected",
        })
        .collect();
    omitted.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    omitted
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectorInfo {
    pub stack_id: &'static str,
    pub schema: u32,
}

#[must_use]
pub fn detector_registry() -> Vec<DetectorInfo> {
    crate::discover::detector_entries()
        .into_iter()
        .map(|(stack_id, schema)| DetectorInfo { stack_id, schema })
        .collect()
}

pub const PLAN_JSON_NAME: &str = "plan.json";

pub fn plan_artifact_dir(velnor_dir: &Path, run_key: &str) -> Result<PathBuf, OrchestratorError> {
    validate_run_key(run_key).map_err(internal_contract)?;
    let dir = velnor_dir.to_string_lossy();
    join_runner_temp(dir.as_ref(), run_key)
        .map(PathBuf::from)
        .map_err(internal_contract)
}

pub fn plan_json_path(velnor_dir: &Path, run_key: &str) -> Result<PathBuf, OrchestratorError> {
    Ok(plan_artifact_dir(velnor_dir, run_key)?.join(PLAN_JSON_NAME))
}

#[must_use]
pub fn dedupe_sorted(ids: &[String]) -> (Vec<String>, Vec<String>) {
    let mut sorted = ids.to_vec();
    sorted.sort();
    let mut unique = Vec::with_capacity(sorted.len());
    let mut duplicates = Vec::new();
    for id in sorted {
        if unique.last().is_some_and(|last: &String| *last == id) {
            duplicates.push(id);
        } else {
            unique.push(id);
        }
    }
    (unique, duplicates)
}

static PREVIEW_COUNTER: AtomicU64 = AtomicU64::new(0);

#[must_use]
pub fn preview_unique_dir(prefix: &str) -> PathBuf {
    let count = PREVIEW_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{count}", std::process::id()))
}
