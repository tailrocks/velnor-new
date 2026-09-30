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
    let archive = NextestArchive::new(driver, &group.package_name, &group.features, target)
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
mod tests {
    use super::reuse_stages::{ExpectedReuseIdentity, ObservedRestoreMeta};
    use super::*;
    use velnor_actions_mise::CachedTaskDescriptor;

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
        let outcome = |availability, eligible: bool| {
            plan_reuse_outcome(
                &group,
                WorkflowEvent::PullRequest,
                availability,
                &digest,
                &digest,
                eligible,
            )
            .expect("outcome")
        };
        let missing = outcome(ToolAvailability::Missing, true);
        assert_eq!(missing.reason, "unproven");
        assert!(!missing.task_cache_enabled);
        let gated = outcome(ToolAvailability::Unqualified, true);
        assert_eq!(gated.reason, MissReason::FORCED_UNCACHED.as_str());
        assert!(!gated.task_cache_enabled);
        let eligible = outcome(ToolAvailability::Ready, true);
        assert_eq!(eligible.reason, MissReason::NO_ENTRY.as_str());
        assert!(!eligible.task_cache_enabled);
        let refused = outcome(ToolAvailability::Ready, false);
        assert_eq!(refused.reason, MissReason::TASK_NOT_ELIGIBLE.as_str());
        for result in [&missing, &gated, &eligible, &refused] {
            assert_eq!(result.decision, ObligationDecision::Execute);
            assert!(result.task_cache_key.is_none());
        }
        let mut dirty = group;
        dirty.undeclared_reads = true;
        assert!(reuse_qualification(&dirty, WorkflowEvent::Push).always_run());
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
    fn archive_gate_binds_sources_and_refuses_unbound() {
        let digest = digest_b3(b"d");
        let source = digest_b3(b"package-sources");
        let plain = group(TaskKind::Nextest, "stack/rust/root/nextest/default");
        let gate = check_archive_identity(&plain, &digest, &digest, &digest);
        assert!(matches!(gate, Ok(ArchiveGate::Clear)));
        let mut sharded = group(
            TaskKind::Nextest,
            "stack/rust/root/nextest/default/shard-1-of-2",
        );
        sharded.compile_driver = "bogus".to_owned();
        let err = check_archive_identity(&sharded, &digest, &digest, &digest).expect_err("driver");
        assert!(err.to_string().contains("unknown_archive_driver"), "{err}");
        sharded.compile_driver = "cargo".to_owned();
        let check = |group: &TaskGroup, source: Option<&str>| {
            check_archive_identity_with_source(group, &digest, &digest, &digest, source)
        };
        let unbound = check(&sharded, None);
        assert!(matches!(unbound, Ok(ArchiveGate::SourceUnbound)));
        let bound = check(&sharded, Some(&source));
        assert!(matches!(bound, Ok(ArchiveGate::Clear)));
        let bogus = check(&sharded, Some("bogus"));
        assert!(matches!(bogus, Ok(ArchiveGate::SourceUnbound)));
        let malformed = group(TaskKind::Nextest, "stack/rust/root/nextest/default/shard-x");
        let err = check(&malformed, Some(&source)).expect_err("malformed");
        assert!(err.to_string().contains("malformed_shard_suffix"), "{err}");
    }

    #[test]
    fn observations_thread_to_verified_or_precise_miss() {
        let bytes = b"report-bytes".to_vec();
        let digest = digest_b3(&bytes);
        let observed = vec![("out/report.json".to_owned(), bytes, digest)];
        let descriptor = CachedTaskDescriptor {
            task_name: "clippy".to_owned(),
            sources: vec!["Cargo.toml".to_owned()],
            outputs: vec!["out/report.json".to_owned()],
            command_inputs: Vec::new(),
            env: std::collections::BTreeMap::new(),
            tools: vec!["rust@1.98.1".to_owned()],
            dep_keys: Vec::new(),
        };
        let (key, compat) = (digest_b3(b"key"), digest_b3(b"compat"));
        let expected = ExpectedReuseIdentity {
            cache_key: key.clone(),
            compatibility_id: compat.clone(),
            owner_scope: "trusted".to_owned(),
        };
        let declared = vec!["out/report.json".to_owned()];
        let good = ObservedRestoreMeta {
            key: key.clone(),
            compat: compat.clone(),
            owner: "trusted".to_owned(),
        };
        let bad = ObservedRestoreMeta {
            key,
            compat: digest_b3(b"other"),
            owner: "trusted".to_owned(),
        };
        let run = |restore: &ObservedRestoreMeta| {
            verify_reused_pipeline(
                &declared,
                &observed,
                Some(&descriptor),
                Some(&expected),
                Some(restore),
            )
        };
        assert!(run(&good).is_ok());
        assert_eq!(
            run(&bad).expect_err("compat"),
            MissReason::COMPATIBILITY_MISMATCH
        );
    }
}
