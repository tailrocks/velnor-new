#[test]
fn advisory_needs_fresh_external_data() {
    let advisory = "stack/rust/root/advisory/default";
    let mut plan = plan_with(&[advisory]);
    let unchanged = Some(BTreeSet::new());
    let tmp = tempfile::tempdir().expect("tempdir");
    seed_sources(tmp.path());
    let catalog = ToolCatalog::pinned();
    let discovery = discovery_with(&[advisory]);
    let live = live_closure_digest(tmp.path(), &discovery, advisory, &catalog);
    let manifest = manifest_with(&[(advisory, &live)]);
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance_for(&manifest),
        &discovery,
        unchanged.as_ref(),
        &inputs(tmp.path(), &catalog),
    );
    assert_eq!(covered, 0);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("external_data_rerun")),
        "{:?}",
        plan.warnings
    );
}

/// Tofu proposal via the T12 adapter constructor.
fn tofu_proposal(kind: velnor_actions_tofu::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu::TofuTaskGroup {
        root: String::new(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// Cover-time bundle for one task over an empty discovery.
fn cover_bundle_for(
    snapshot: &ExecutionSnapshot,
    discovery: &Discovery,
    task: &ProposedTask,
    root: &std::path::Path,
) -> crate::internal_plan::identities::ExtensionBundle {
    extension_bundle_with_snapshot(snapshot, discovery, task, Some(root), None)
}

/// Valid tofu spellings verify at cover time (G5 close).
#[test]
fn tofu_cover_extension_verifies_valid_spellings() {
    use velnor_actions_tofu::TofuTaskKind;
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join(".terraform.lock.hcl"), "lock").expect("lockfile");
    let task = tofu_proposal(TofuTaskKind::Validate);
    let discovery = discovery_with(&[]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = cover_bundle_for(&snapshot, &discovery, &task, tmp.path());
    let mut reads = velnor_actions_tofu::FileCache::new();
    verify_cover_extension(&task, tmp.path(), &bundle, &mut reads)
        .expect("tofu verifies at cover time");
}

/// Drifted tofu spellings refuse coverage, never parse loosely.
#[test]
fn tofu_cover_extension_refuses_drift() {
    use velnor_actions_tofu::TofuTaskKind;
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut task = tofu_proposal(TofuTaskKind::InitForValidate);
    task.identity.compile_driver = "cargo".to_owned();
    let discovery = discovery_with(&[]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = cover_bundle_for(&snapshot, &discovery, &task, tmp.path());
    let mut reads = velnor_actions_tofu::FileCache::new();
    let err =
        verify_cover_extension(&task, tmp.path(), &bundle, &mut reads).expect_err("drift refuses");
    assert!(err.contains("unparsable_spelling"), "{err}");
}

/// Undeclared tofu inputs refuse coverage like undeclared rust reads.
#[test]
fn tofu_cover_extension_refuses_undeclared_inputs() {
    use velnor_actions_tofu::TofuTaskKind;
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut task = tofu_proposal(TofuTaskKind::Validate);
    task.identity.undeclared_reads = true;
    let discovery = discovery_with(&[]);
    let snapshot = ExecutionSnapshot::build(&discovery);
    let bundle = cover_bundle_for(&snapshot, &discovery, &task, tmp.path());
    let mut reads = velnor_actions_tofu::FileCache::new();
    let err = verify_cover_extension(&task, tmp.path(), &bundle, &mut reads)
        .expect_err("undeclared refuses");
    assert_eq!(err, "undeclared_inputs");
}
