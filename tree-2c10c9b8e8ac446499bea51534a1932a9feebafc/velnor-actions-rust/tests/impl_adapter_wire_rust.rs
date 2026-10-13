//! Adapter-wire cases: extension slots, payload argv/env, gates, fmt scope,
//! derive signals, consumption, and index modes (F1R/F2 rust halves).
use velnor_actions_contract::{
    ContractError, IndexMode, build_index_from_tracked, digest_b3, validate_rust_extension,
};
use velnor_actions_rust::tasks::{
    DigestSlot, ExtensionInputs, RustTaskIdentityExtension, TaskGroup, TaskKind, cargo_payload_argv,
};
use velnor_actions_rust::{
    CompileDriver, DeriveInputs, GroupExtensionInputs, NextestProfile, PackageRecord,
    ProfileSource, RUSTDOCFLAGS_ENV, RustExecutionProfile, TargetRecord, TestRunner,
    cargo_payload_env, derive_task_groups, derive_workspace_fmt_if_explicit,
};

fn target(kind: &str) -> TargetRecord {
    TargetRecord {
        kind: kind.to_owned(),
        name: "a".to_owned(),
        test: true,
        doctest: kind == "lib",
        required_features: Vec::new(),
    }
}

fn package() -> PackageRecord {
    PackageRecord {
        id: "a-id".to_owned(),
        name: "a".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: "crates/a/Cargo.toml".to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![target("lib"), target("bin")],
        features: vec!["default".to_owned()],
        has_build_script: true,
    }
}

fn profile() -> RustExecutionProfile {
    RustExecutionProfile {
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        evidence: Vec::new(),
        driver_source: ProfileSource::Detected,
        runner_source: ProfileSource::Detected,
        nextest_profile: NextestProfile::Default,
        nextest_config: None,
        run_ignored: None,
    }
}

fn groups_for(package: &PackageRecord) -> Result<Vec<TaskGroup>, ContractError> {
    let profile = profile();
    let features = vec!["default".to_owned()];
    let inputs = DeriveInputs {
        package,
        profile: &profile,
        configuration: "default",
        features: &features,
        target: "host",
        explicit_fmt: false,
    };
    derive_task_groups(&inputs)
}

fn group(kind: TaskKind) -> TaskGroup {
    TaskGroup {
        task_id: "t".to_owned(),
        package_id: "p".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind,
        configuration: "default".to_owned(),
        features: vec!["default".to_owned()],
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
        nextest_profile: NextestProfile::Default,
    }
}

fn text(group: &TaskGroup) -> Result<Vec<String>, ContractError> {
    Ok(cargo_payload_argv(group)?
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect())
}

#[test]
fn extension_carries_workspace_and_profile() {
    let targets = vec!["lib".to_owned()];
    let features = vec!["default".to_owned()];
    let declared: Vec<String> = Vec::new();
    let graph = digest_b3(b"graph");
    let config = digest_b3(b"config");
    let inputs = ExtensionInputs {
        package_id: "demo 0.1.0",
        workspace_id: "workspace",
        profile: "default",
        manifest: "Cargo.toml",
        graph_digest: &graph,
        targets: &targets,
        features: &features,
        target: "host",
        driver: CompileDriver::Cargo,
        runner: TestRunner::CargoTest,
        config_digest: &config,
        lock_digest: DigestSlot::Unknown("unprobed".to_owned()),
        nextest_digest: DigestSlot::Unknown("unprobed".to_owned()),
        kind: TaskKind::Test,
        archive_source: None,
        rerun_inputs: Some(&[]),
        has_build_script: false,
        declared_inputs: &declared,
    };
    let ext = RustTaskIdentityExtension::for_task(&inputs);
    assert_eq!(ext.workspace_id, "workspace");
    assert_eq!(ext.profile, "default");
    assert_eq!(validate_rust_extension(&ext.to_stack_extension()), Ok(()));
    let group = group(TaskKind::Test);
    let via_group = group.identity_extension(&GroupExtensionInputs {
        package_id: "demo 0.1.0",
        workspace_id: "workspace",
        profile: "default",
        manifest: "Cargo.toml",
        graph_digest: &graph,
        targets: &targets,
        config_digest: &config,
        lock_digest: DigestSlot::Unknown("unprobed".to_owned()),
        nextest_digest: DigestSlot::Unknown("unprobed".to_owned()),
        archive_source: None,
        rerun_inputs: Some(&[]),
        has_build_script: false,
    });
    assert_eq!(
        validate_rust_extension(&via_group.to_stack_extension()),
        Ok(())
    );
}

#[test]
fn clippy_denies_warnings_after_all_targets() -> Result<(), ContractError> {
    let argv = text(&group(TaskKind::Clippy))?;
    let tail = argv.iter().map(String::as_str).collect::<Vec<_>>();
    assert!(tail.ends_with(&["--all-targets", "--", "-D", "warnings"]));
    Ok(())
}

#[test]
fn test_kinds_scope_to_package() -> Result<(), ContractError> {
    for kind in [
        TaskKind::Test,
        TaskKind::Nextest,
        TaskKind::Doctest,
        TaskKind::Doc,
        TaskKind::Build,
    ] {
        let argv = text(&group(kind))?;
        assert!(
            argv.windows(2).any(|w| w == ["--package", "demo"]),
            "{kind:?} misses --package"
        );
    }
    assert!(!text(&group(TaskKind::Fmt))?.contains(&"--package".to_owned()));
    Ok(())
}

#[test]
fn nextest_fails_on_no_tests() -> Result<(), ContractError> {
    let argv = text(&group(TaskKind::Nextest))?;
    let tail = argv.iter().map(String::as_str).collect::<Vec<_>>();
    assert!(tail.ends_with(&["--no-tests", "fail"]));
    Ok(())
}

#[test]
fn doc_env_denies_warnings() {
    let env = cargo_payload_env(TaskKind::Doc);
    assert_eq!(env.len(), 1);
    assert_eq!(env[0].0.to_string_lossy(), RUSTDOCFLAGS_ENV);
    assert_eq!(env[0].1.to_string_lossy(), "-D warnings");
    for kind in [
        TaskKind::Fmt,
        TaskKind::Clippy,
        TaskKind::Test,
        TaskKind::Build,
    ] {
        assert!(cargo_payload_env(kind).is_empty(), "{kind:?} carries env");
    }
}

#[test]
fn doc_gated_by_doctest_and_clippy() -> Result<(), ContractError> {
    let package = package();
    let groups = groups_for(&package)?;
    let clippy = groups
        .iter()
        .find(|g| g.kind == TaskKind::Clippy)
        .expect("clippy");
    let doctest = groups
        .iter()
        .find(|g| g.kind == TaskKind::Doctest)
        .expect("doctest");
    let doc = groups
        .iter()
        .find(|g| g.kind == TaskKind::Doc)
        .expect("doc");
    assert_eq!(
        doc.gated_by,
        vec![clippy.task_id.clone(), doctest.task_id.clone()]
    );
    assert_eq!(doctest.gated_by, vec![clippy.task_id.clone()]);
    Ok(())
}

#[test]
fn workspace_fmt_only_with_explicit_config() {
    let profile = profile();
    let none = derive_workspace_fmt_if_explicit("Cargo.toml", &profile, "default", "host", false)
        .expect("optional");
    assert!(none.is_none());
    let some = derive_workspace_fmt_if_explicit("Cargo.toml", &profile, "default", "host", true)
        .expect("optional");
    let group = some.expect("group");
    assert_eq!(group.kind, TaskKind::Fmt);
}

#[test]
fn derive_marks_build_script_reads_undeclared() -> Result<(), ContractError> {
    let package = package();
    let groups = groups_for(&package)?;
    assert!(!groups.is_empty());
    for group in &groups {
        assert!(group.undeclared_reads, "{} misses signal", group.task_id);
    }
    let mut plain = package;
    plain.has_build_script = false;
    plain.targets = vec![target("lib")];
    for group in groups_for(&plain)? {
        assert!(!group.undeclared_reads, "{} over-marks", group.task_id);
    }
    Ok(())
}

#[test]
fn rust_payloads_declare_no_nondeterminism() -> Result<(), ContractError> {
    let package = package();
    for group in groups_for(&package)? {
        assert!(!group.uses_network && !group.uses_clock && !group.uses_random);
    }
    Ok(())
}

#[test]
fn consumes_matches_declared_inputs() {
    let group = group(TaskKind::Test)
        .with_declared_inputs(&["proto/a.proto".to_owned()])
        .expect("inputs");
    assert!(group.consumes("proto/a.proto"));
    assert!(!group.consumes("proto/b.proto"));
}

#[test]
fn index_modes_select_tracked_and_untracked() {
    let temp = crate::support::TempDir::create("wire-index").expect("tempdir");
    let root = temp.path();
    let tracked = vec!["src/a.rs".to_owned()];
    let untracked = vec!["src/b.rs".to_owned()];
    let exclusions: Vec<String> = Vec::new();
    let both = build_index_from_tracked(root, &tracked, &untracked, IndexMode::Both, &exclusions)
        .expect("both");
    assert_eq!(
        both.files(),
        &["src/a.rs".to_owned(), "src/b.rs".to_owned()]
    );
    let only_tracked =
        build_index_from_tracked(root, &tracked, &untracked, IndexMode::Tracked, &exclusions)
            .expect("tracked");
    assert_eq!(only_tracked.files(), &["src/a.rs".to_owned()]);
    let only_untracked = build_index_from_tracked(
        root,
        &tracked,
        &untracked,
        IndexMode::Untracked,
        &exclusions,
    )
    .expect("untracked");
    assert_eq!(only_untracked.files(), &["src/b.rs".to_owned()]);
}
