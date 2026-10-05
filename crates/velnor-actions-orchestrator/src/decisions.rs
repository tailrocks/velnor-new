//! Planner decisions: classification, selection, and artifact paths.
//!
//! Pure coordination policy shared by plan and merge: metadata-failure
//! classification, three-way obligation decisions, `not_selected`
//! report creation, broaden-path classification, the omission ledger,
//! the detector registry, plan-artifact paths, baseline expiry, and
//! duplicate detection. Exact-base run selection lives in
//! [`crate::run_select`]. The restore-ownership gate is test-only;
//! production reuse verifies ownership through the staged pipeline
//! instead.

// Unit tests live apart so `decisions.rs` keeps its size gate.
#[cfg(test)]
#[path = "decisions_tests.rs"]
mod decisions_tests;

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

/// Why one manifest produced no inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataFailure {
    /// Malformed manifest or inventory: select or ignore the candidate.
    Malformed,
    /// Incomplete discovery (offline deps, tooling): fail preparation.
    Incomplete,
}

/// Offline indicators forcing `preparation_incomplete`, never a fetch.
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

/// Classify cargo-metadata stderr: offline/tooling or malformed (arch §4).
#[must_use]
pub fn classify_metadata_failure(stderr: &str) -> MetadataFailure {
    let lower = stderr.to_lowercase();
    if OFFLINE_MARKERS.iter().any(|mark| lower.contains(mark)) {
        MetadataFailure::Incomplete
    } else {
        MetadataFailure::Malformed
    }
}

/// Task-cache hit evidence for one obligation decision.
///
/// A bare bool cannot carry a hit: only [`CacheHit::Verified`] reuses,
/// constructed solely alongside the verified restore evidence it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheHit {
    /// No verified hit; execute instead.
    Execute,
    /// A verified task-cache hit is in hand.
    Verified,
}

/// Inputs for one three-way obligation decision (par §5).
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "five independent decision signals read clearest as named bools"
)]
pub struct ObligationInputs {
    /// Task carries a known adapter extension schema.
    pub schema_known: bool,
    /// Build reads undeclared inputs.
    pub undeclared_inputs: bool,
    /// Network/clock/random/service effects.
    pub nondeterministic: bool,
    /// Dynamic inputs are controlled and recorded.
    pub inputs_controlled: bool,
    /// Verified task-cache hit evidence, or execution.
    pub cache_hit: CacheHit,
    /// Trusted-baseline proof covers the obligation.
    pub baseline_covered: bool,
    /// Precise restore miss reason, if a restore was attempted.
    pub restore_miss_reason: Option<&'static str>,
}

/// Classify one obligation `execute`/`reused`/`covered` (par §5).
///
/// Unknown schemas, undeclared inputs, and uncontrolled dynamic effects
/// always execute; a verified hit reuses; a baseline proof covers; a
/// restore miss executes with its precise reason.
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
    if inputs.baseline_covered {
        return (
            ObligationDecision::CoveredByTrustedBaseline,
            "covered_by_trusted_baseline",
        );
    }
    if let Some(reason) = inputs.restore_miss_reason {
        return (ObligationDecision::Execute, reason);
    }
    (ObligationDecision::Execute, "selected")
}

/// Owner scope for one trust value: the only live scopes that exist.
///
/// The gate takes [`Trust`], never a caller-supplied string, so no
/// future caller can bless a restore under an invented scope.
#[cfg(test)]
fn owner_scope_for_trust(trust: Trust) -> &'static str {
    match trust {
        Trust::Trusted => "trusted",
        Trust::Pr => "pr",
    }
}

/// Restore classification with ownership as an explicit check (cache §2).
///
/// Test-only: no production path consumes `RestoreObservation` yet
/// (task reports carry no restore observations; the staged reuse
/// pipeline verifies ownership separately), so this gate pins the
/// intended semantics for the future call site instead of enforcing
/// anything today. The live trust scope must match the observed owner
/// first (`ownership_mismatch`); otherwise the Mise ordered checks over
/// observed evidence (present, digest, compat, trust, inputs) decide.
///
/// Observations are caller-constructed: only real restore I/O may
/// build them, since a hand-written literal with self-consistent
/// digests would classify clean. Sealing the type (private fields
/// plus a fallible I/O constructor) is deferred to the owning Mise
/// lane; every future call site needs constructor review until then.
///
/// # Errors
///
/// Returns the precise miss reason when ownership or any check fails.
#[cfg(test)]
pub fn classify_restore_with_ownership(
    trust: Trust,
    obs: &velnor_actions_mise::restore_evidence::RestoreObservation,
) -> Result<(), &'static str> {
    if obs.observed_owner != owner_scope_for_trust(trust) {
        return Err("ownership_mismatch");
    }
    velnor_actions_mise::restore_evidence::classify_restore(obs)
}

/// Inputs for one `not_selected` report (task §6).
#[derive(Debug, Clone)]
pub struct NotSelectedInputs<'a> {
    /// Run key.
    pub run_key: &'a str,
    /// Triggering event.
    pub event: WorkflowEvent,
    /// Trust scope.
    pub trust: Trust,
    /// Owning matrix ID.
    pub matrix_id: &'a str,
    /// Immutable runner placement from the matrix plan.
    pub planned_platform: &'a PlannedPlatform,
    /// Matrix key.
    pub matrix_key: &'a str,
    /// Skipped task ID.
    pub task_id: &'a str,
    /// Skipped task digest.
    pub task_digest: &'a str,
    /// Skip reason.
    pub reason: NotSelectedReason,
}

/// Create a validated `not_selected` report for a skipped obligation.
///
/// # Errors
///
/// Returns a contract error when the derived report ID fails validation.
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

/// Runner-image evidence at generation time (VER-4.2).
///
/// Explicitly unobserved: the generator never sees the provisioned
/// runner, so label text is never split into `ImageOS`/`ImageVersion`
/// facts. Observed provisioner values bind later through
/// [`RunnerImageEvidence::observed`], which rejects this marker, so
/// the two states stay disjoint by construction (P03-4).
pub(crate) fn runner_image_evidence() -> RunnerImageEvidence {
    RunnerImageEvidence::unobserved()
}

/// Broaden warning for global-config or outside-project paths, if any.
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

/// Broadening class one changed path triggers, unioned over stacks.
///
/// Each stack classifies its own lock/root-config paths; the orchestrator
/// only unions the verdicts and owns the warning vocabulary. Tofu
/// contributes nothing by design (T12 call): every tofu lockfile lives
/// under its root, so lockfile changes attribute per-root through
/// calling-root selection instead of broadening; the `[stacks.tofu]`
/// table itself broadens as global config via
/// [`selection_broadens_for_path`].
#[must_use]
pub(crate) fn broadening_for_path(path: &str) -> Option<SelectionBroadening> {
    for stack in Stack::all() {
        match stack {
            Stack::Rust => {
                if let Some(class) = velnor_actions_rust::selection_broadening(path) {
                    return Some(class);
                }
            }
            Stack::Tofu | Stack::Mise => {}
        }
    }
    None
}

/// One omitted task plus its internal-record explanation (arch §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskOmission {
    /// Omitted task ID.
    pub task_id: String,
    /// Omission reason.
    pub reason: &'static str,
}

/// Omission ledger: every unselected task with a reason, sorted.
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

/// One registered detector: stack ID plus record schema (par §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectorInfo {
    /// Registered stack ID.
    pub stack_id: &'static str,
    /// Detection record schema.
    pub schema: u32,
}

/// Detector registry in ascending stack-ID order; V1 holds `rust` and `tofu`.
#[must_use]
pub fn detector_registry() -> Vec<DetectorInfo> {
    crate::discover::detector_entries()
        .into_iter()
        .map(|(stack_id, schema)| DetectorInfo { stack_id, schema })
        .collect()
}

/// Plan-file name under the run-key artifact directory (wf §4).
pub const PLAN_JSON_NAME: &str = "plan.json";

/// Validated `<velnor-dir>/<run-key>/` artifact directory.
///
/// # Errors
///
/// Returns an internal error when the run key is invalid.
pub fn plan_artifact_dir(velnor_dir: &Path, run_key: &str) -> Result<PathBuf, OrchestratorError> {
    validate_run_key(run_key).map_err(internal_contract)?;
    let dir = velnor_dir.to_string_lossy();
    join_runner_temp(dir.as_ref(), run_key)
        .map(PathBuf::from)
        .map_err(internal_contract)
}

/// Validated `$RUNNER_TEMP/velnor/<run-key>/plan.json` path.
///
/// # Errors
///
/// Returns an internal error when the run key is invalid.
pub fn plan_json_path(velnor_dir: &Path, run_key: &str) -> Result<PathBuf, OrchestratorError> {
    Ok(plan_artifact_dir(velnor_dir, run_key)?.join(PLAN_JSON_NAME))
}

/// True when a baseline expired at or before `now_unix` (par §10).
#[must_use]
pub fn baseline_expired(expires_at_unix: Option<u64>, now_unix: u64) -> bool {
    expires_at_unix.is_some_and(|expiry| now_unix >= expiry)
}

/// Sorted unique IDs plus sorted duplicates (par §3 dedupe stage).
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

/// Monotonic suffix keeping preview dirs unique per process.
static PREVIEW_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Unique absent preview root under the system temp dir (wf §1).
#[must_use]
pub fn preview_unique_dir(prefix: &str) -> PathBuf {
    let count = PREVIEW_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{count}", std::process::id()))
}
