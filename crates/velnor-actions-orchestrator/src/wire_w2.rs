//! FLOW-half wiring: reuse, graph, archive, and baseline-download helpers.
//!
//! Pure coordination glue for the cross-crate TODO notes: task-cache reuse
//! qualification (REUSE-1..3, REUSE-6..7), plan-graph construction (PAR-3.1,
//! PAR-3.2), archive identity (PAR-8.3), and exact-name baseline downloads
//! (PAR-5.10, PAR-5.5). Declared via `#[path]` from `internal_plan.rs`.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    ObligationDecision, PlanGenerator, WorkflowEvent, cachekey, digest_b3,
};
use velnor_actions_mise::cache::mode_for_event;
use velnor_actions_mise::restore::{
    MissReason, RestoreEvidence, ReusePlan, fallback_for_error, plan_reuse,
    verify_restored_task_result,
};
use velnor_actions_mise::{
    ArchiveIdentityInputs, CachedTaskDescriptor, NextestArchive, NextestDriver, PinnedTool,
    ReuseQualification, ReuseSignal, TaskCacheKey, TaskReuseRequest, ToolAvailability, ToolCatalog,
    archive_identity,
};
use velnor_actions_rust::{TaskGroup, TaskKind};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::manifest_for_key;

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

/// Cache-key trust scope for one workflow event.
fn trust_name(event: WorkflowEvent) -> &'static str {
    match event {
        WorkflowEvent::Push | WorkflowEvent::MergeGroup => "trusted",
        WorkflowEvent::PullRequest | WorkflowEvent::Fork | WorkflowEvent::Local => "pr",
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
        ReusePlan::Reuse(grant) => {
            let descriptor = reuse_descriptor(group);
            let request = TaskReuseRequest::new(descriptor, &[], grant)
                .map_err(|err| internal(&format!("task_reuse_rejected:{err}")))?;
            let key = TaskCacheKey::derive(request.grant(), request.descriptor());
            let digest = match key {
                Ok(key) => key.digest().to_owned(),
                Err(err) => {
                    let fallback = fallback_for_error(&err);
                    return Ok(ReuseOutcome {
                        decision: ObligationDecision::Execute,
                        reason: fallback.as_str().to_owned(),
                        task_cache_key: None,
                        task_cache_enabled: false,
                    });
                }
            };
            let _read_key =
                cachekey::cache_key("task", trust_name(event), toolchain_id, input_digest)
                    .map_err(internal_contract)?;
            Ok(ReuseOutcome {
                decision: ObligationDecision::ReusedFromTaskCache,
                reason: "reused_from_task_cache".to_owned(),
                task_cache_key: Some(digest),
                task_cache_enabled: true,
            })
        }
        ReusePlan::Execute(fallback) => Ok(ReuseOutcome {
            decision: ObligationDecision::Execute,
            reason: fallback.reason().as_str().to_owned(),
            task_cache_key: None,
            task_cache_enabled: false,
        }),
    }
}

/// Declared cache-key inputs: the kind-addressed task keyed by its manifest.
fn reuse_descriptor(group: &TaskGroup) -> CachedTaskDescriptor {
    CachedTaskDescriptor {
        task_name: group.kind.as_str().to_owned(),
        sources: vec![manifest_for_key(&group.manifest_key)],
        outputs: Vec::new(),
        command_inputs: Vec::new(),
        env: BTreeMap::new(),
        tools: Vec::new(),
        dep_keys: Vec::new(),
    }
}

/// Archive identity gate for sharded Nextest groups (PAR-8.3).
///
/// Unsharded groups run in place and need no archive; sharded groups must
/// carry well-formed archive inputs over the real platform/config digests.
pub(crate) fn check_archive_identity(
    group: &TaskGroup,
    toolchain_id: &str,
    platform_id: &str,
    config_digest: &str,
) -> Result<(), OrchestratorError> {
    if group.kind != TaskKind::Nextest || !group.task_id.contains("/shard-") {
        return Ok(());
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
    let source_digest = digest_b3(manifest_for_key(&group.manifest_key).as_bytes());
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
