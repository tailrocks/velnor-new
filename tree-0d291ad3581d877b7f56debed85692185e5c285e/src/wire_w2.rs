//! FLOW-half wiring: reuse qualification, archive identity (PAR-8.3).
//!
//! Pure coordination glue, declared via `#[path]` from `internal_plan.rs`.

// Wired here so reuse stages compile without touching `lib.rs`.
#[path = "reuse_stages.rs"]
pub(crate) mod reuse_stages;

use velnor_actions_contract::{
    ObligationDecision, PlanGenerator, WorkflowEvent, digest_b3, validate_digest,
};
use velnor_actions_mise::{
    ArchiveIdentityInputs, NextestArchive, NextestDriver, PinnedTool, ReuseQualification,
    ReuseSignal, ToolAvailability, ToolCatalog, archive_identity,
};
use velnor_actions_mise::{
    cache::mode_for_event,
    restore::{MissReason, ReusePlan, plan_reuse},
};
use velnor_actions_rust::{TaskGroup, TaskKind};

use self::reuse_stages::verify_reused_pipeline;
use crate::{OrchestratorError, internal::internal};
/// Wiring inputs for one group: event plus generator identity.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GroupWire<'a> {
    pub(crate) event: WorkflowEvent,
    pub(crate) generator: &'a PlanGenerator,
}

/// Reuse-cache event name for one workflow event.
pub(crate) fn reuse_event_name(event: WorkflowEvent) -> &'static str {
    match event {
        WorkflowEvent::PullRequest | WorkflowEvent::Fork => "pull_request",
        WorkflowEvent::Push => "push",
        WorkflowEvent::MergeGroup => "merge_group",
        WorkflowEvent::Local => "local",
    }
}

/// Qualification for one group: kind plus event plus adapter signals.
pub(crate) fn reuse_qualification(group: &TaskGroup, event: WorkflowEvent) -> ReuseQualification {
    let mut qualification = ReuseQualification::new(group.kind.as_str(), reuse_event_name(event));
    for (flag, signal) in [
        (group.uses_network, ReuseSignal::Network),
        (group.uses_clock, ReuseSignal::Clock),
        (group.uses_random, ReuseSignal::Random),
        (group.undeclared_reads, ReuseSignal::UndeclaredState),
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
}
impl ReuseOutcome {
    fn execute_with(reason: String) -> Self {
        Self {
            decision: ObligationDecision::Execute,
            reason,
            task_cache_key: None,
            task_cache_enabled: false,
        }
    }
    /// Forced execution with an explicit reason (changed work).
    pub(crate) fn execute(reason: &str) -> Self {
        Self::execute_with(reason.to_owned())
    }
}
/// Decide reuse eligibility for one group (REUSE-1/6/7, PAR-3.4).
/// Plan time never grants reuse: merge-time evidence required, so eligible
/// groups execute with `no_entry` and task-cache stays disabled.
pub(crate) fn plan_reuse_outcome(
    group: &TaskGroup,
    event: WorkflowEvent,
    availability: ToolAvailability,
    toolchain_id: &str,
    input_digest: &str,
    reuse_eligible: bool,
) -> Result<ReuseOutcome, OrchestratorError> {
    validate_digest(toolchain_id).map_err(|err| internal(&err.to_string()))?;
    validate_digest(input_digest).map_err(|err| internal(&err.to_string()))?;
    if !reuse_eligible {
        return Ok(ReuseOutcome::execute_with(
            MissReason::TASK_NOT_ELIGIBLE.as_str().to_owned(),
        ));
    }
    match availability {
        ToolAvailability::Ready => {}
        ToolAvailability::Unqualified => {
            return Ok(ReuseOutcome::execute_with(
                MissReason::FORCED_UNCACHED.as_str().to_owned(),
            ));
        }
        ToolAvailability::Missing => {
            return Ok(ReuseOutcome::execute_with("unproven".to_owned()));
        }
    }
    let mode = mode_for_event(reuse_event_name(event))
        .map_err(|err| internal(&format!("reuse_mode_rejected:{err}")))?;
    match plan_reuse(availability, &reuse_qualification(group, event), mode) {
        ReusePlan::Reuse(_) => Ok(ReuseOutcome::execute_with(
            MissReason::NO_ENTRY.as_str().to_owned(),
        )),
        ReusePlan::Execute(fallback) => Ok(ReuseOutcome::execute_with(
            fallback.reason().as_str().to_owned(),
        )),
    }
}

/// Shard index/count from a `/shard-<index>-of-<count>` task-ID suffix.
fn parse_shard_suffix(task_id: &str) -> Option<(u32, u32)> {
    let (_, shard) = task_id.split_once("/shard-")?;
    if shard.contains('/') {
        return None;
    }
    let (index, count) = shard.split_once("-of-")?;
    let (index, count) = (index.parse::<u32>().ok()?, count.parse::<u32>().ok()?);
    if index == 0 || count == 0 || index > count {
        return None;
    }
    Some((index, count))
}

/// Archive-gate verdict for one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArchiveGate {
    /// No archive to verify, or the archive identity verified.
    Clear,
    /// Sharded archive with unbound source: execute with reason, never abort.
    SourceUnbound,
}

/// Archive gate for sharded Nextest groups; unbound sources refuse
/// the task (execute with reason) instead of aborting the plan.
pub(crate) fn check_archive_identity(
    group: &TaskGroup,
    toolchain_id: &str,
    platform_id: &str,
    config_digest: &str,
) -> Result<ArchiveGate, OrchestratorError> {
    check_archive_identity_with_source(group, toolchain_id, platform_id, config_digest, None)
}

/// Archive gate with an explicit content-bound source digest.
pub(crate) fn check_archive_identity_with_source(
    group: &TaskGroup,
    toolchain_id: &str,
    platform_id: &str,
    config_digest: &str,
    source_digest: Option<&str>,
) -> Result<ArchiveGate, OrchestratorError> {
    if group.kind != TaskKind::Nextest {
        return Ok(ArchiveGate::Clear);
    }
    if !group.task_id.contains("/shard-") {
        return Ok(ArchiveGate::Clear);
    }
    if parse_shard_suffix(&group.task_id).is_none() {
        return Err(internal(&format!(
            "malformed_shard_suffix:{}",
            group.task_id
        )));
    }
    let driver = match group.compile_driver.as_str() {
        "cargo" => NextestDriver::Cargo,
        "mbx" => NextestDriver::Mbx,
        other => return Err(internal(&format!("unknown_archive_driver:{other}"))),
    };
    let marker = digest_b3(crate::internal_plan::snapshot::UNKNOWN_ARCHIVE_SOURCE.as_bytes());
    let Some(source) =
        source_digest.filter(|digest| validate_digest(digest).is_ok() && *digest != marker)
    else {
        return Ok(ArchiveGate::SourceUnbound);
    };
    let target = (group.target != "host").then_some(group.target.as_str());
    let archive = NextestArchive::with_profile(
        driver,
        &group.package_name,
        &group.features,
        target,
        &group.nextest_profile,
    )
    .map_err(|err| internal(&format!("archive_inputs_rejected:{err}")))?;
    let catalog = ToolCatalog::pinned();
    let inputs = ArchiveIdentityInputs {
        source_digest: source,
        profile: &group.configuration,
        toolchain_id,
        runtime: &group.compile_driver,
        test_runner: catalog.version(PinnedTool::Nextest),
        format: "tar.zst",
        platform_id,
        config_digest,
    };
    archive_identity(&archive, &inputs)
        .map(|_| ArchiveGate::Clear)
        .map_err(|err| internal(&format!("archive_identity_rejected:{err}")))
}

/// Verify a `reused` task; fails closed without restore observations.
/// `Verified` needs [`verify_reused_pipeline`] with all three observation
/// arguments once reports carry restore metadata.
pub(crate) fn verify_reused_task(
    _task_id: &str,
    declared: &[String],
    observed: &[(String, Vec<u8>, String)],
) -> Result<(), MissReason> {
    verify_reused_pipeline(declared, observed, None, None, None)
}
#[cfg(test)]
#[path = "wire_w2_tests.rs"]
mod wire_w2_tests;
