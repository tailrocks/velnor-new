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
    /// Forced execution with an explicit reason (changed work).
    pub(crate) fn execute(reason: &str) -> Self {
        Self {
            decision: ObligationDecision::Execute,
            reason: reason.to_owned(),
            task_cache_key: None,
            task_cache_enabled: false,
        }
    }
}

/// Decide reuse for one group (REUSE-1/6/7, PAR-3.4).
///
/// Unqualified extensions never reuse; unavailable tools keep the
/// `Execute`/`unproven` outcome; granted reuse builds the typed request
/// (REUSE-2, fail-closed) and records the cache-key digest (REUSE-3).
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
    if availability != ToolAvailability::Ready {
        return Ok(ReuseOutcome {
            decision: ObligationDecision::Execute,
            reason: "unproven".to_owned(),
            task_cache_key: None,
            task_cache_enabled: false,
        });
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

/// Archive gate with an explicit content-bound source digest.
///
/// Sharded Nextest groups clear only on a content-bound source; unbound
/// sources refuse the task (execute with reason) instead of aborting
/// the plan.
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

/// Verify a `reused` task against its restore evidence (REUSE-5).
///
/// Reuse needs observed output digests covering every declared output;
/// a claim with no observations fails closed as incomplete, never
/// passes on intact evidence alone. Observed digests need task-level
/// cache outcomes in the report contract (P04).
pub(crate) fn verify_reused_task(
    task_id: &str,
    declared: &[String],
    observed: &[(String, Vec<u8>, String)],
) -> Result<(), MissReason> {
    if observed.is_empty() {
        return Err(MissReason::TASK_RESULT_INCOMPLETE);
    }
    verify_restored_task_result(task_id, RestoreEvidence::intact(), declared, observed)
}
#[cfg(test)]
mod tests {
    use velnor_actions_mise::restore::RestoreCheck;

    use super::*;

    /// Minimal group with kind, task ID, and nondeterminism flags.
    fn group(kind: TaskKind, task_id: &str) -> TaskGroup {
        TaskGroup {
            task_id: task_id.to_owned(),
            package_id: "demo".to_owned(),
            package_name: "demo".to_owned(),
            manifest_key: "root".to_owned(),
            kind,
            configuration: "default".to_owned(),
            features: Vec::new(),
            target: "host".to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: None,
            compile_driver: "cargo".to_owned(),
            test_runner: "cargo_nextest".to_owned(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
        }
    }

    #[test]
    fn reuse_outcomes_cover_missing_ready_and_refused() {
        let group = group(TaskKind::Nextest, "stack/rust/root/nextest/default");
        let digest = digest_b3(b"toolchain");
        let missing = plan_reuse_outcome(
            &group,
            WorkflowEvent::PullRequest,
            ToolAvailability::Missing,
            &digest,
            &digest,
            true,
        )
        .expect("missing");
        assert_eq!(missing.decision, ObligationDecision::Execute);
        assert_eq!(missing.reason, "unproven");
        assert!(missing.task_cache_key.is_none());
        let granted = plan_reuse_outcome(
            &group,
            WorkflowEvent::PullRequest,
            ToolAvailability::Ready,
            &digest,
            &digest,
            true,
        )
        .expect("granted");
        assert_eq!(granted.decision, ObligationDecision::ReusedFromTaskCache);
        assert!(granted.task_cache_enabled);
        assert!(
            granted
                .task_cache_key
                .is_some_and(|key| velnor_actions_contract::validate_digest(&key).is_ok())
        );
        let mut dirty = group;
        dirty.undeclared_reads = true;
        let refused = plan_reuse_outcome(
            &dirty,
            WorkflowEvent::PullRequest,
            ToolAvailability::Ready,
            &digest,
            &digest,
            false,
        )
        .expect("refused");
        assert_eq!(refused.decision, ObligationDecision::Execute);
        assert_eq!(refused.reason, MissReason::TASK_NOT_ELIGIBLE.as_str());
        let signals = reuse_qualification(&dirty, WorkflowEvent::Push);
        assert!(signals.always_run());
        assert_eq!(
            verify_reused_task("t", &[], &[]).expect_err("no observations"),
            MissReason::TASK_RESULT_INCOMPLETE
        );
        let broken = RestoreEvidence::intact().fail(RestoreCheck::EntryPresent);
        assert_eq!(
            verify_restored_task_result("t", broken, &[], &[]).expect_err("entry"),
            MissReason::NO_ENTRY
        );
    }

    #[test]
    fn archive_gate_skips_unsharded_and_rejects_unknown_driver() {
        let digest = digest_b3(b"d");
        let plain = group(TaskKind::Nextest, "stack/rust/root/nextest/default");
        assert!(check_archive_identity(&plain, &digest, &digest, &digest).is_ok());
        let mut sharded = group(
            TaskKind::Nextest,
            "stack/rust/root/nextest/default/shard-1-of-2",
        );
        sharded.compile_driver = "bogus".to_owned();
        let err = check_archive_identity(&sharded, &digest, &digest, &digest).expect_err("driver");
        assert!(err.to_string().contains("unknown_archive_driver"), "{err}");
        sharded.compile_driver = "cargo".to_owned();
        assert!(check_archive_identity(&sharded, &digest, &digest, &digest).is_ok());
    }
}
