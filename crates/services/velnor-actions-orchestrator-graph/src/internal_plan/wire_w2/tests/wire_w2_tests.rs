use super::*;

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
        assert_eq!(
            result.recorded_input_digest.as_deref(),
            Some(digest.as_str())
        );
    }
    assert_eq!(
        ReuseOutcome::execute("affected_by_change").recorded_input_digest,
        None
    );
    let mut dirty = group;
    dirty.identity.undeclared_reads = true;
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

/// Tofu proposal via the T12 adapter constructor.
fn tofu_group(kind: velnor_actions_tofu_core::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu_core::TofuTaskGroup {
        root: String::new(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu_core::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// T21: task-result reuse stays disabled for tofu init/validate: plan
/// time executes with precise reasons and never enables task-cache,
/// and init must-run carries the Network signal on top.
#[test]
fn tofu_init_and_validate_never_plan_reuse() {
    use velnor_actions_tofu_core::TofuTaskKind;
    let digest = digest_b3(b"toolchain");
    let outcome = |task: &ProposedTask, availability| {
        plan_reuse_outcome(
            task,
            WorkflowEvent::PullRequest,
            availability,
            &digest,
            &digest,
            true,
        )
        .expect("outcome")
    };
    let init = tofu_group(TofuTaskKind::InitForValidate);
    assert!(init.resource.needs_network);
    assert!(reuse_qualification(&init, WorkflowEvent::Push).always_run());
    let blocked = outcome(&init, ToolAvailability::Ready);
    assert_eq!(blocked.reason, MissReason::TASK_NOT_ELIGIBLE.as_str());
    let validate = tofu_group(TofuTaskKind::Validate);
    let refused = outcome(&validate, ToolAvailability::Ready);
    assert_eq!(refused.reason, MissReason::NO_ENTRY.as_str());
    for result in [
        blocked,
        refused,
        outcome(&init, ToolAvailability::Unqualified),
        outcome(&validate, ToolAvailability::Unqualified),
    ] {
        assert_eq!(result.decision, ObligationDecision::Execute);
        assert!(!result.task_cache_enabled);
        assert!(result.task_cache_key.is_none());
    }
}

#[test]
fn reused_tasks_verify_outputs_then_fail_trust_closed() {
    let run = |task: &str,
               declared: &[String],
               observed: &[(String, Vec<u8>, String)]|
     -> Result<(), MissReason> {
        verify_reused_task(task, declared, observed, None, None, None)
    };
    assert_eq!(
        run("t", &[], &[]).expect_err("no observations"),
        MissReason::TASK_RESULT_INCOMPLETE
    );
    let declared = vec!["out/report.json".to_owned()];
    assert_eq!(
        run("t", &declared, &[]).expect_err("no payload"),
        MissReason::TASK_RESULT_INCOMPLETE
    );
    let bytes = b"report-bytes".to_vec();
    let digest = digest_b3(&bytes);
    let observed = vec![("out/report.json".to_owned(), bytes.clone(), digest.clone())];
    assert_eq!(
        run("t", &[], &observed).expect_err("empty descriptor"),
        MissReason::TASK_NOT_ELIGIBLE
    );
    let tampered = vec![("out/report.json".to_owned(), bytes, digest_b3(b"other"))];
    assert_eq!(
        run("t", &declared, &tampered).expect_err("tampered"),
        MissReason::CACHE_CORRUPT
    );
    let empty = vec![("out/report.json".to_owned(), Vec::new(), digest.clone())];
    assert_eq!(
        run("t", &declared, &empty).expect_err("zero byte"),
        MissReason::TASK_RESULT_INCOMPLETE
    );
    assert_eq!(
        run("t", &declared, &observed).expect_err("no trust anchor"),
        MissReason::TASK_RESULT_INCOMPLETE
    );
}

#[test]
fn reuse_event_names_match_mode_spellings() {
    use velnor_actions_mise::cache::{TaskCacheMode, mode_for_event};
    assert_eq!(reuse_event_name(WorkflowEvent::Fork), "fork");
    assert_eq!(reuse_event_name(WorkflowEvent::PullRequest), "pull_request");
    for event in [
        WorkflowEvent::PullRequest,
        WorkflowEvent::Fork,
        WorkflowEvent::Push,
        WorkflowEvent::MergeGroup,
        WorkflowEvent::Local,
    ] {
        let name = reuse_event_name(event);
        assert!(mode_for_event(name).is_ok(), "{name}");
    }
    assert_eq!(
        mode_for_event(reuse_event_name(WorkflowEvent::Fork)).expect("fork"),
        TaskCacheMode::ReadOnly
    );
}

#[test]
fn archive_gate_binds_sources_and_refuses_unbound() {
    let digest = digest_b3(b"d");
    let source = digest_b3(b"package-sources");
    let plain = group(TaskKind::Nextest, "stack/rust/root/nextest/default");
    let gate = check_archive_identity_with_source(&plain, &digest, &digest, &digest, None);
    assert!(matches!(gate, Ok(ArchiveGate::Clear)));
    let mut sharded = group(
        TaskKind::Nextest,
        "stack/rust/root/nextest/default/shard-1-of-2",
    );
    sharded.identity.compile_driver = velnor_actions_rust_core::CompileDriver::Cargo
        .as_str()
        .to_owned();
    let check = |task: &ProposedTask, source: Option<&str>| {
        check_archive_identity_with_source(task, &digest, &digest, &digest, source)
    };
    let unbound = check(&sharded, None);
    assert!(matches!(unbound, Ok(ArchiveGate::SourceUnbound)));
    let bound = check(&sharded, Some(&source));
    assert!(matches!(bound, Ok(ArchiveGate::Clear)));
    let bogus = check(&sharded, Some("bogus"));
    assert!(matches!(bogus, Ok(ArchiveGate::SourceUnbound)));
    let mut malformed = group(
        TaskKind::Nextest,
        "stack/rust/root/nextest/default/shard-1-of-2",
    );
    malformed.task_id = "stack/rust/root/nextest/default/shard-x".to_owned();
    let err = check(&malformed, Some(&source)).expect_err("malformed");
    assert!(err.to_string().contains("malformed_shard_suffix"), "{err}");
}

#[test]
fn archive_identity_binds_content_not_path() {
    let digest = digest_b3(b"d");
    let source_before = digest_b3(b"package-sources-v1");
    let source_after = digest_b3(b"package-sources-v2");
    let archive = NextestArchive::with_profile(NextestDriver::Cargo, "demo", &[], None, "default")
        .expect("archive");
    let catalog = ToolCatalog::pinned();
    let identity = |source: &str| {
        archive_identity(
            &archive,
            &ArchiveIdentityInputs {
                source_digest: source,
                profile: "default",
                toolchain_id: &digest,
                runtime: "cargo",
                test_runner: catalog.version(PinnedTool::Nextest),
                format: "tar.zst",
                platform_id: &digest,
                config_digest: &digest,
            },
        )
        .expect("identity")
    };
    // Same configuration and path, changed source bytes: the identity
    // must flip, so a same-path edit can never silently reuse an
    // archive built from stale bytes.
    assert_ne!(identity(&source_before), identity(&source_after));
    assert_eq!(identity(&source_before), identity(&source_before));
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
    let live = digest_b3(b"inputs");
    let expected = ExpectedReuseIdentity {
        cache_key: key.clone(),
        compatibility_id: compat.clone(),
        owner_scope: "trusted".to_owned(),
        input_digest: live.clone(),
    };
    let declared = vec!["out/report.json".to_owned()];
    let task = "stack/rust/root/clippy/default";
    let good = ObservedRestoreMeta {
        task_id: task.to_owned(),
        key: key.clone(),
        compat: compat.clone(),
        owner: "trusted".to_owned(),
    };
    let bad = ObservedRestoreMeta {
        task_id: task.to_owned(),
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
            task,
            Some(&live),
        )
    };
    assert!(run(&good).is_ok());
    assert_eq!(
        run(&bad).expect_err("compat"),
        MissReason::COMPATIBILITY_MISMATCH
    );
}

/// Bound task plus live digest: foreign evidence and drifted inputs
/// reject, while the fully bound reuse verifies.
#[test]
fn verified_reuse_binds_task_and_live_inputs() {
    let bytes = b"report-bytes".to_vec();
    let digest = digest_b3(&bytes);
    let observed = vec![("out/report.json".to_owned(), bytes, digest)];
    let (key, compat) = (digest_b3(b"key"), digest_b3(b"compat"));
    let live = digest_b3(b"inputs");
    let expected = ExpectedReuseIdentity {
        cache_key: key.clone(),
        compatibility_id: compat.clone(),
        owner_scope: "trusted".to_owned(),
        input_digest: live.clone(),
    };
    let declared = vec!["out/report.json".to_owned()];
    let task = "stack/rust/root/clippy/default";
    let good = ObservedRestoreMeta {
        task_id: task.to_owned(),
        key,
        compat,
        owner: "trusted".to_owned(),
    };
    let foreign = ObservedRestoreMeta {
        task_id: "stack/rust/root/other/default".to_owned(),
        ..good.clone()
    };
    assert_eq!(
        verify_reused_task(
            task,
            &declared,
            &observed,
            Some(&expected),
            Some(&foreign),
            Some(&live)
        )
        .expect_err("task"),
        MissReason::NO_ENTRY
    );
    assert_eq!(
        verify_reused_task(
            task,
            &declared,
            &observed,
            Some(&expected),
            Some(&good),
            Some(&digest_b3(b"edited")),
        )
        .expect_err("live drift"),
        MissReason::INPUT_DIGEST_MISMATCH
    );
    assert!(
        verify_reused_task(
            task,
            &declared,
            &observed,
            Some(&expected),
            Some(&good),
            Some(&live)
        )
        .is_ok()
    );
}
