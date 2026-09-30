//! FLOW-half wiring: reuse qualification, archive identity (PAR-8.3).
//!
//! Pure coordination glue, declared via `#[path]` from `internal_plan.rs`.

// Wired here so reuse stages compile without touching `lib.rs`.
#[path = "reuse_stages.rs"]
pub(crate) mod reuse_stages;

use velnor_actions_contract::{
    ObligationDecision, PlanGenerator, WorkflowEvent, digest_b3, validate_digest,
};
use velnor_actions_mise::cache::mode_for_event;
use velnor_actions_mise::restore::{MissReason, ReusePlan, plan_reuse};
use velnor_actions_mise::{
    ArchiveIdentityInputs, NextestArchive, NextestDriver, PinnedTool, ReuseQualification,
    ReuseSignal, ToolAvailability, ToolCatalog, archive_identity,
};
use velnor_actions_rust::{TaskGroup, TaskKind};

use self::reuse_stages::verify_reused_pipeline;
use crate::OrchestratorError;
use crate::internal::internal;
use crate::internal_plan::snapshot::UNKNOWN_ARCHIVE_SOURCE;

/// Wiring inputs for one group: event plus generator identity.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GroupWire<'a> {
    /// Workflow event driving reuse.
    pub(crate) event: WorkflowEvent,
    /// Generator identity for digests.
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
    /// Obligation decision.
    pub(crate) decision: ObligationDecision,
    /// Precise decision reason.
    pub(crate) reason: String,
    /// Task-cache key digest, when reuse was granted.
    pub(crate) task_cache_key: Option<String>,
    /// Gate-6 task-layer enablement for the obligation.
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

/// Decide reuse eligibility for one group (REUSE-1/6/7, PAR-3.4).
///
/// Plan time establishes eligibility only: presence, restore, and
/// verification need execution evidence that exists solely at merge
/// time, so this never grants `ReusedFromTaskCache` (merge rejects
/// plan-time reuse claims). Eligible groups execute with `no_entry`;
/// the pinned Mise task-cache feature stays disabled until Gate-6
/// qualification plus the opaque transport exist (follow-up).
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
        return Ok(ReuseOutcome {
            decision: ObligationDecision::Execute,
            reason: MissReason::TASK_NOT_ELIGIBLE.as_str().to_owned(),
            task_cache_key: None,
            task_cache_enabled: false,
        });
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
        ReusePlan::Reuse(_) => Ok(ReuseOutcome {
            decision: ObligationDecision::Execute,
            reason: MissReason::NO_ENTRY.as_str().to_owned(),
            task_cache_key: None,
            task_cache_enabled: false,
        }),
        ReusePlan::Execute(fallback) => Ok(ReuseOutcome {
            decision: ObligationDecision::Execute,
            reason: fallback.reason().as_str().to_owned(),
            task_cache_key: None,
            task_cache_enabled: false,
        }),
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

/// Archive identity gate for sharded Nextest groups (PAR-8.3).
///
/// Unsharded groups run in place and need no archive; sharded groups
/// must carry well-formed archive inputs over the real platform/config
/// digests. The source slot commits to the explicit unknown marker:
/// content bytes are unavailable at plan time, so pathnames never
/// stand in for content (content binding is a follow-up).
pub(crate) fn check_archive_identity(
    group: &TaskGroup,
    toolchain_id: &str,
    platform_id: &str,
    config_digest: &str,
) -> Result<(), OrchestratorError> {
    if group.kind != TaskKind::Nextest {
        return Ok(());
    }
    if !group.task_id.contains("/shard-") {
        return Ok(());
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
    let target = if group.target == "host" {
        None
    } else {
        Some(group.target.as_str())
    };
    let archive = NextestArchive::new(driver, &group.package_name, &group.features, target)
        .map_err(|err| internal(&format!("archive_inputs_rejected:{err}")))?;
    let catalog = ToolCatalog::pinned();
    let source_digest = digest_b3(UNKNOWN_ARCHIVE_SOURCE.as_bytes());
    let inputs = ArchiveIdentityInputs {
        source_digest: &source_digest,
        profile: &group.configuration,
        toolchain_id,
        runtime: &group.compile_driver,
        test_runner: catalog.version(PinnedTool::Nextest),
        format: "tar.zst",
        platform_id,
        config_digest,
    };
    archive_identity(&archive, &inputs)
        .map(|_| ())
        .map_err(|err| internal(&format!("archive_identity_rejected:{err}")))
}

/// Verify a `reused` task through the reuse stages (REUSE-5).
///
/// Runs the full presence/eligibility/restored/verified pipeline over
/// the declared and observed outputs. Merge-time calls carry no
/// descriptor, expectations, or restore metadata, so verification of
/// outputs is real but the trust gate always fails closed until the
/// report contract carries restore observations (P04 follow-up).
pub(crate) fn verify_reused_task(
    _task_id: &str,
    declared: &[String],
    observed: &[(String, Vec<u8>, String)],
) -> Result<(), MissReason> {
    verify_reused_pipeline(declared, observed, None, None, None)
}

#[cfg(test)]
mod tests {
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
    fn reuse_outcomes_execute_with_precise_reasons() {
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
        assert!(!missing.task_cache_enabled);
        let eligible = plan_reuse_outcome(
            &group,
            WorkflowEvent::PullRequest,
            ToolAvailability::Ready,
            &digest,
            &digest,
            true,
        )
        .expect("eligible");
        assert_eq!(eligible.decision, ObligationDecision::Execute);
        assert_eq!(eligible.reason, MissReason::NO_ENTRY.as_str());
        assert!(!eligible.task_cache_enabled);
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
        assert!(
            plan_reuse_outcome(
                &dirty,
                WorkflowEvent::PullRequest,
                ToolAvailability::Ready,
                "bogus",
                &digest,
                true,
            )
            .is_err()
        );
    }

    #[test]
    fn reused_tasks_verify_outputs_then_fail_trust_closed() {
        assert_eq!(
            verify_reused_task("t", &[], &[]).expect_err("no observations"),
            MissReason::TASK_RESULT_INCOMPLETE
        );
        let declared = vec!["out/report.json".to_owned()];
        assert_eq!(
            verify_reused_task("t", &declared, &[]).expect_err("no payload"),
            MissReason::TASK_RESULT_INCOMPLETE
        );
        let bytes = b"report-bytes".to_vec();
        let digest = digest_b3(&bytes);
        let observed = vec![("out/report.json".to_owned(), bytes.clone(), digest.clone())];
        assert_eq!(
            verify_reused_task("t", &[], &observed).expect_err("empty descriptor"),
            MissReason::TASK_NOT_ELIGIBLE
        );
        let tampered = vec![("out/report.json".to_owned(), bytes, digest_b3(b"other"))];
        assert_eq!(
            verify_reused_task("t", &declared, &tampered).expect_err("tampered"),
            MissReason::CACHE_CORRUPT
        );
        let empty = vec![("out/report.json".to_owned(), Vec::new(), digest.clone())];
        assert_eq!(
            verify_reused_task("t", &declared, &empty).expect_err("zero byte"),
            MissReason::TASK_RESULT_INCOMPLETE
        );
        assert_eq!(
            verify_reused_task("t", &declared, &observed).expect_err("no trust anchor"),
            MissReason::TASK_RESULT_INCOMPLETE
        );
    }

    #[test]
    fn archive_gate_validates_shards_without_path_sources() {
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
        let malformed = group(TaskKind::Nextest, "stack/rust/root/nextest/default/shard-x");
        let err =
            check_archive_identity(&malformed, &digest, &digest, &digest).expect_err("malformed");
        assert!(err.to_string().contains("malformed_shard_suffix"), "{err}");
    }
}
