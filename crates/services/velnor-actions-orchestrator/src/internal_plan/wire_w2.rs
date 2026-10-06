//! FLOW-half wiring: reuse qualification, archive identity (PAR-8.3).
//!
//! Pure coordination glue, declared via `#[path]` from `internal_plan.rs`.

// Wired here so reuse stages compile without touching `lib.rs`.
pub(crate) mod reuse_stages;

use velnor_actions_contract::{
    ObligationDecision, PlanGenerator, ProposedTask, WorkflowEvent, digest_b3, validate_digest,
};
use velnor_actions_mise::{
    ArchiveIdentityInputs, NextestArchive, NextestDriver, PinnedTool, ReuseQualification,
    ReuseSignal, ToolAvailability, ToolCatalog, archive_identity,
};
use velnor_actions_mise::{
    cache::mode_for_event,
    restore::MissReason,
    reuse::{ReusePlan, plan_reuse},
};
use velnor_actions_rust::is_nextest_kind;
use velnor_actions_rust_core::CompileDriver;

use self::reuse_stages::{ExpectedReuseIdentity, ObservedRestoreMeta, verify_reused_pipeline};
use crate::{OrchestratorError, internal::internal};
/// Wiring inputs for one task: event plus generator identity.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GroupWire<'a> {
    pub(crate) event: WorkflowEvent,
    pub(crate) generator: &'a PlanGenerator,
}

/// Reuse-cache event name for one workflow event.
///
/// Spellings match [`mode_for_event`](velnor_actions_mise::cache::mode_for_event)
/// exactly: forks stay `fork` (read-only), never the `pull_request`
/// spelling, so the two layers can never disagree about fork handling.
pub(crate) fn reuse_event_name(event: WorkflowEvent) -> &'static str {
    match event {
        WorkflowEvent::PullRequest => "pull_request",
        WorkflowEvent::Fork => "fork",
        WorkflowEvent::Push => "push",
        WorkflowEvent::MergeGroup => "merge_group",
        WorkflowEvent::Local => "local",
    }
}

/// Qualification for one task: kind plus event plus adapter signals.
pub(crate) fn reuse_qualification(task: &ProposedTask, event: WorkflowEvent) -> ReuseQualification {
    let mut qualification = ReuseQualification::new(&task.task_kind, reuse_event_name(event));
    for (flag, signal) in [
        (task.resource.needs_network, ReuseSignal::Network),
        (task.uses_clock, ReuseSignal::Clock),
        (task.uses_random, ReuseSignal::Random),
        (task.identity.undeclared_reads, ReuseSignal::UndeclaredState),
    ] {
        if flag {
            qualification = qualification.with_signal(signal);
        }
    }
    qualification
}

/// Reuse outcome for one plan obligation.
pub(crate) struct ReuseOutcome {
    pub(crate) decision: ObligationDecision,
    pub(crate) reason: String,
    pub(crate) task_cache_key: Option<String>,
    pub(crate) task_cache_enabled: bool,
    /// Validated plan-time input digest the merge-time live digest must
    /// still equal; `None` only for forced-execution paths that never
    /// validated a digest.
    pub(crate) recorded_input_digest: Option<String>,
}
impl ReuseOutcome {
    fn execute_with(reason: String, recorded_input_digest: Option<String>) -> Self {
        Self {
            decision: ObligationDecision::Execute,
            reason,
            task_cache_key: None,
            task_cache_enabled: false,
            recorded_input_digest,
        }
    }
    /// Forced execution with an explicit reason (changed work).
    pub(crate) fn execute(reason: &str) -> Self {
        Self::execute_with(reason.to_owned(), None)
    }
}
/// Decide reuse eligibility for one task (REUSE-1/6/7, PAR-3.4).
/// Plan time never grants reuse: merge-time evidence required, so eligible
/// tasks execute with `no_entry` and task-cache stays disabled. The
/// validated input digest is recorded on the outcome so the persisted
/// obligation carries the exact value the merge-time live digest must
/// still equal (same-path source edits flip the live digest and reject).
pub(crate) fn plan_reuse_outcome(
    task: &ProposedTask,
    event: WorkflowEvent,
    availability: ToolAvailability,
    toolchain_id: &str,
    input_digest: &str,
    reuse_eligible: bool,
) -> Result<ReuseOutcome, OrchestratorError> {
    validate_digest(toolchain_id).map_err(|err| internal(&err.to_string()))?;
    validate_digest(input_digest).map_err(|err| internal(&err.to_string()))?;
    let recorded = Some(input_digest.to_owned());
    if !reuse_eligible {
        return Ok(ReuseOutcome::execute_with(
            MissReason::TASK_NOT_ELIGIBLE.as_str().to_owned(),
            recorded,
        ));
    }
    match availability {
        ToolAvailability::Ready => {}
        ToolAvailability::Unqualified => {
            return Ok(ReuseOutcome::execute_with(
                MissReason::FORCED_UNCACHED.as_str().to_owned(),
                recorded,
            ));
        }
        ToolAvailability::Missing => {
            return Ok(ReuseOutcome::execute_with("unproven".to_owned(), recorded));
        }
    }
    let mode = mode_for_event(reuse_event_name(event))
        .map_err(|err| internal(&format!("reuse_mode_rejected:{err}")))?;
    match plan_reuse(availability, &reuse_qualification(task, event), mode) {
        ReusePlan::Reuse(_) => Ok(ReuseOutcome::execute_with(
            MissReason::NO_ENTRY.as_str().to_owned(),
            recorded,
        )),
        ReusePlan::Execute(fallback) => Ok(ReuseOutcome::execute_with(
            fallback.reason().as_str().to_owned(),
            recorded,
        )),
    }
}

/// Shard index/count from a `/shard-<index>-of-<count>` task-ID suffix.
fn parse_shard_suffix(task_id: &str) -> Option<(u32, u32)> {
    let (_, index, count) = velnor_actions_contract::split_shard_suffix(task_id)?;
    Some((index, count))
}

/// Archive-gate verdict for one task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveGate {
    /// No archive to verify, or the archive identity verified.
    Clear,
    /// Sharded archive with unbound source: execute with reason, never abort.
    SourceUnbound,
}

/// Archive gate with an explicit content-bound source digest.
///
/// Sharded Nextest tasks clear only on a content-bound source; unbound
/// sources refuse the task (execute with reason) instead of aborting
/// the plan.
pub(crate) fn check_archive_identity_with_source(
    task: &ProposedTask,
    toolchain_id: &str,
    platform_id: &str,
    config_digest: &str,
    source_digest: Option<&str>,
) -> Result<ArchiveGate, OrchestratorError> {
    if !is_nextest_kind(&task.task_kind) {
        return Ok(ArchiveGate::Clear);
    }
    if !task.task_id.contains("/shard-") {
        return Ok(ArchiveGate::Clear);
    }
    if parse_shard_suffix(&task.task_id).is_none() {
        return Err(internal(&format!(
            "malformed_shard_suffix:{}",
            task.task_id
        )));
    }
    let driver = match CompileDriver::parse(&task.identity.compile_driver)? {
        CompileDriver::Cargo => NextestDriver::Cargo,
        CompileDriver::Mbx => NextestDriver::Mbx,
    };
    let marker = digest_b3(crate::internal_plan::snapshot::UNKNOWN_ARCHIVE_SOURCE.as_bytes());
    let Some(source) =
        source_digest.filter(|digest| validate_digest(digest).is_ok() && *digest != marker)
    else {
        return Ok(ArchiveGate::SourceUnbound);
    };
    let target = (task.identity.target != "host").then_some(task.identity.target.as_str());
    let archive = NextestArchive::with_profile(
        driver,
        &task.display_name,
        &task.identity.features,
        target,
        &task.runner_profile,
    )
    .map_err(|err| internal(&format!("archive_inputs_rejected:{err}")))?;
    let catalog = ToolCatalog::pinned();
    let inputs = ArchiveIdentityInputs {
        source_digest: source,
        profile: &task.configuration,
        toolchain_id,
        runtime: &task.identity.compile_driver,
        test_runner: catalog.version(PinnedTool::Nextest),
        format: "tar.zst",
        platform_id,
        config_digest,
    };
    archive_identity(&archive, &inputs)
        .map(|_| ArchiveGate::Clear)
        .map_err(|err| internal(&format!("archive_identity_rejected:{err}")))
}

/// Verify a `reused` task against its bound identity and live inputs.
///
/// The restore evidence must name `task_id`, and the live input digest
/// must still equal the recorded plan-time digest; missing evidence of
/// any kind fails closed. `Verified` needs the full observation set
/// once reports carry restore metadata.
pub(crate) fn verify_reused_task(
    task_id: &str,
    declared: &[String],
    observed: &[(String, Vec<u8>, String)],
    expected: Option<&ExpectedReuseIdentity>,
    restore: Option<&ObservedRestoreMeta>,
    live_input_digest: Option<&str>,
) -> Result<(), MissReason> {
    verify_reused_pipeline(
        declared,
        observed,
        None,
        expected,
        restore,
        task_id,
        live_input_digest,
    )
}
#[cfg(test)]
mod tests;
