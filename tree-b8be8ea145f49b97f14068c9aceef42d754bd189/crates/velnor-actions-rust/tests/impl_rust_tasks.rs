//! Task-group derivation cases.
use velnor_actions_contract::validate_task_id;
use velnor_actions_rust::{
    CompileDriver, DeriveInputs, NextestProfile, PackageRecord, ProfileSource,
    RustExecutionProfile, TargetRecord, TaskKind, TestRunner, derive_task_groups,
    derive_workspace_fmt,
};

/// Target entry for fixtures.
fn target(kind: &str, name: &str, test: bool, doctest: bool) -> TargetRecord {
    TargetRecord {
        kind: kind.to_owned(),
        name: name.to_owned(),
        test,
        doctest,
        required_features: Vec::new(),
    }
}

/// Sample package with lib, bins, tests, examples, benches, and build script.
fn package() -> PackageRecord {
    PackageRecord {
        id: "a-id".to_owned(),
        name: "a".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: "crates/a/Cargo.toml".to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![
            target("lib", "a", true, true),
            target("bin", "a-bin", true, false),
            target("test", "integration", true, false),
            target("example", "demo", true, false),
            target("bench", "criterion", false, false),
            target("custom-build", "build-script-build", false, false),
        ],
        features: vec!["default".to_owned()],
        has_build_script: true,
        v1_task_owner: velnor_actions_rust::VelnorV1TaskOwner::Project,
    }
}

/// Package without test-bearing targets.
fn empty_package() -> PackageRecord {
    let mut p = package();
    p.targets = vec![target("custom-build", "build-script-build", false, false)];
    p
}

/// Binary-only package: testable bins but no doctest-able lib target.
fn bin_only_package() -> PackageRecord {
    let mut p = package();
    p.targets = vec![
        target("bin", "a-bin", true, false),
        target("test", "integration", true, false),
    ];
    p.has_build_script = false;
    p
}

fn cargo_profile() -> RustExecutionProfile {
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

fn nextest_profile() -> RustExecutionProfile {
    let mut p = cargo_profile();
    p.compile_driver = CompileDriver::Mbx;
    p.test_runner = TestRunner::CargoNextest;
    p.nextest_profile = NextestProfile::Ci;
    p.nextest_config = Some(".config/nextest.toml".to_owned());
    p
}

/// Derivation inputs for `package` under `profile`.
fn inputs<'a>(
    package: &'a PackageRecord,
    profile: &'a RustExecutionProfile,
    features: &'a [String],
) -> DeriveInputs<'a> {
    DeriveInputs {
        package,
        profile,
        configuration: "default",
        features,
        target: "host",
        explicit_fmt: false,
    }
}

/// Task ids of `groups` for `kind`.
fn ids_for(groups: &[velnor_actions_rust::TaskGroup], kind: TaskKind) -> Vec<&str> {
    groups
        .iter()
        .filter(|group| group.kind == kind)
        .map(|group| group.task_id.as_str())
        .collect()
}

#[test]
fn derives_groups_with_clippy_gates() {
    let package = package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let inputs = inputs(&package, &profile, &features);
    let groups = derive_task_groups(&inputs);
    let groups = groups.expect("derivation ok");
    let kinds: Vec<TaskKind> = groups.iter().map(|group| group.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TaskKind::Clippy,
            TaskKind::Test,
            TaskKind::Doctest,
            TaskKind::Doc
        ]
    );
    for group in &groups {
        assert!(validate_task_id(&group.task_id).is_ok());
        assert!(group.task_id.starts_with("stack/rust/crates/a/"));
        assert_eq!(group.manifest_key, "crates/a");
        assert_eq!(group.package_id, "a-id");
    }
    let clippy = &groups[0];
    assert_eq!(clippy.gated_by, [] as [std::string::String; 0]);
    assert_eq!(clippy.depends_on, [] as [std::string::String; 0]);
    for group in groups.iter().skip(1) {
        if group.kind == TaskKind::Doc {
            let doctest = groups
                .iter()
                .find(|g| g.kind == TaskKind::Doctest)
                .expect("doctest");
            assert_eq!(
                group.gated_by,
                vec![clippy.task_id.clone(), doctest.task_id.clone()]
            );
        } else {
            assert_eq!(group.gated_by, vec![clippy.task_id.clone()]);
        }
    }
    assert_eq!(groups[1].depends_on, [] as [std::string::String; 0]);
}

#[test]
fn nextest_adds_build_with_data_edge() {
    let package = package();
    let profile = nextest_profile();
    let features = vec!["default".to_owned()];
    let inputs = inputs(&package, &profile, &features);
    let groups = derive_task_groups(&inputs).expect("derivation ok");
    let kinds: Vec<TaskKind> = groups.iter().map(|group| group.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TaskKind::Clippy,
            TaskKind::Build,
            TaskKind::Nextest,
            TaskKind::Doctest,
            TaskKind::Doc
        ]
    );
    assert_eq!(ids_for(&groups, TaskKind::Test), [] as [&str; 0]);
    let clippy = ids_for(&groups, TaskKind::Clippy);
    let build = ids_for(&groups, TaskKind::Build);
    let nextest: Vec<&velnor_actions_rust::TaskGroup> = groups
        .iter()
        .filter(|group| group.kind == TaskKind::Nextest)
        .collect();
    assert_eq!(nextest.len(), 1);
    assert_eq!(nextest[0].gated_by, vec![clippy[0].to_owned()]);
    assert_eq!(nextest[0].depends_on, vec![build[0].to_owned()]);
}

#[test]
fn cargo_test_emits_only_existing_target_flags() {
    let package = package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let inputs = inputs(&package, &profile, &features);
    let groups = derive_task_groups(&inputs).expect("derivation ok");
    let test: Vec<&velnor_actions_rust::TaskGroup> = groups
        .iter()
        .filter(|group| group.kind == TaskKind::Test)
        .collect();
    assert_eq!(test.len(), 1);
    assert_eq!(
        test[0].target_flags,
        vec!["--lib", "--bins", "--tests", "--examples"]
    );
    assert!(!test[0].no_test_targets);
    let doctest: Vec<&velnor_actions_rust::TaskGroup> = groups
        .iter()
        .filter(|group| group.kind == TaskKind::Doctest)
        .collect();
    assert_eq!(doctest[0].target_flags, [] as [std::string::String; 0]);
    assert!(!doctest[0].no_test_targets);
}

#[test]
fn no_targets_records_valid_no_test_targets() {
    let package = empty_package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let inputs = inputs(&package, &profile, &features);
    let groups = derive_task_groups(&inputs).expect("derivation ok");
    let test: Vec<&velnor_actions_rust::TaskGroup> = groups
        .iter()
        .filter(|group| group.kind == TaskKind::Test)
        .collect();
    assert!(test[0].no_test_targets);
    assert_eq!(test[0].target_flags, [] as [std::string::String; 0]);
    let doctest: Vec<&velnor_actions_rust::TaskGroup> = groups
        .iter()
        .filter(|group| group.kind == TaskKind::Doctest)
        .collect();
    assert!(doctest[0].no_test_targets);
}

#[test]
fn bin_only_package_marks_doctest_valid_no_target() {
    let bin_only = bin_only_package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let derived = inputs(&bin_only, &profile, &features);
    let groups = derive_task_groups(&derived).expect("derivation ok");
    let doctest: Vec<&velnor_actions_rust::TaskGroup> = groups
        .iter()
        .filter(|group| group.kind == TaskKind::Doctest)
        .collect();
    assert_eq!(doctest.len(), 1);
    assert!(doctest[0].no_test_targets);
    let test: Vec<&velnor_actions_rust::TaskGroup> = groups
        .iter()
        .filter(|group| group.kind == TaskKind::Test)
        .collect();
    assert_eq!(test.len(), 1);
    assert!(!test[0].no_test_targets);
    assert_eq!(test[0].target_flags, vec!["--bins", "--tests"]);
    let with_lib = package();
    let derived = inputs(&with_lib, &profile, &features);
    let groups = derive_task_groups(&derived).expect("derivation ok");
    let doctest: Vec<&velnor_actions_rust::TaskGroup> = groups
        .iter()
        .filter(|group| group.kind == TaskKind::Doctest)
        .collect();
    assert_eq!(doctest.len(), 1);
    assert!(!doctest[0].no_test_targets);
}

#[test]
fn doctest_stays_separate_in_both_profiles() {
    let package = package();
    let features = vec!["default".to_owned()];
    for profile in [cargo_profile(), nextest_profile()] {
        let inputs = inputs(&package, &profile, &features);
        let groups = derive_task_groups(&inputs).expect("derivation ok");
        assert_eq!(ids_for(&groups, TaskKind::Doctest).len(), 1);
    }
    let cargo = cargo_profile();
    let inputs = inputs(&package, &cargo, &features);
    let groups = derive_task_groups(&inputs).expect("derivation ok");
    assert_eq!(ids_for(&groups, TaskKind::Nextest), [] as [&str; 0]);
    assert_eq!(ids_for(&groups, TaskKind::Build), [] as [&str; 0]);
}

#[test]
fn clippy_names_exactly_one_package() {
    let package = package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let inputs = inputs(&package, &profile, &features);
    let groups = derive_task_groups(&inputs).expect("derivation ok");
    let clippy: Vec<&velnor_actions_rust::TaskGroup> = groups
        .iter()
        .filter(|group| group.kind == TaskKind::Clippy)
        .collect();
    assert_eq!(clippy.len(), 1);
    assert_eq!(clippy[0].package_arg.as_deref(), Some("a"));
    for group in &groups {
        if group.kind != TaskKind::Clippy {
            assert!(group.package_arg.is_none());
        }
    }
}

#[test]
fn fmt_only_with_explicit_config() {
    let package = package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let plain = inputs(&package, &profile, &features);
    let groups = derive_task_groups(&plain).expect("derivation ok");
    assert_eq!(ids_for(&groups, TaskKind::Fmt), [] as [&str; 0]);
    let mut explicit = inputs(&package, &profile, &features);
    explicit.explicit_fmt = true;
    let groups = derive_task_groups(&explicit).expect("derivation ok");
    assert_eq!(ids_for(&groups, TaskKind::Fmt).len(), 1);
    let workspace = derive_workspace_fmt("crates/a/Cargo.toml", &profile, "default", "host");
    let workspace = workspace.expect("workspace fmt ok");
    assert_eq!(workspace.task_id, "stack/rust/crates/a/fmt/default");
    assert_eq!(workspace.package_id, "");
    assert_eq!(workspace.gated_by, [] as [std::string::String; 0]);
}

#[test]
fn workspace_fmt_carries_driver_runner() {
    let profile = nextest_profile();
    let workspace = derive_workspace_fmt("Cargo.toml", &profile, "default", "host");
    let workspace = workspace.expect("workspace fmt ok");
    assert_eq!(workspace.compile_driver, CompileDriver::Mbx);
    assert_eq!(workspace.test_runner, TestRunner::CargoNextest);
    assert_eq!(workspace.declared_inputs, [] as [std::string::String; 0]);
}

#[test]
fn declared_inputs_accept_non_rust_and_reject_bad() {
    let package = package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let inputs = inputs(&package, &profile, &features);
    let groups = derive_task_groups(&inputs).expect("derivation ok");
    assert!(groups.iter().all(|group| group.declared_inputs.is_empty()));
    let paths = vec![
        "tests/data/corpus.md".to_owned(),
        "tests/data/vectors.json".to_owned(),
        "tests/data/corpus.md".to_owned(),
    ];
    let with = groups[1].clone().with_declared_inputs(&paths);
    let with = with.expect("declared inputs ok");
    assert_eq!(
        with.declared_inputs,
        vec![
            "tests/data/corpus.md".to_owned(),
            "tests/data/vectors.json".to_owned(),
        ]
    );
    for bad in ["", "/absolute/path.md", "a/../../escape.md"] {
        let attempt = groups[1].clone().with_declared_inputs(&[bad.to_owned()]);
        assert!(attempt.is_err(), "bad input must be rejected: {bad}");
    }
}

#[test]
fn carries_driver_runner_and_sorted_features() {
    let package = package();
    let profile = nextest_profile();
    let features = vec!["zeta".to_owned(), "alpha".to_owned()];
    let inputs = inputs(&package, &profile, &features);
    let groups = derive_task_groups(&inputs).expect("derivation ok");
    for group in &groups {
        assert_eq!(group.compile_driver, CompileDriver::Mbx);
        assert_eq!(group.test_runner, TestRunner::CargoNextest);
        assert_eq!(group.features, vec!["alpha".to_owned(), "zeta".to_owned()]);
        assert_eq!(group.target, "host");
        assert_eq!(group.configuration, "default");
    }
}

#[test]
fn conformance_and_visual_crates_auto_enable_run_ignored() {
    let mut pkg = package();
    pkg.name = "termrock-conformance".to_owned();
    let profile = nextest_profile();
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&inputs(&pkg, &profile, &features)).expect("derivation ok");
    let nextest = groups
        .iter()
        .find(|g| g.kind == TaskKind::Nextest)
        .expect("has nextest");
    assert_eq!(nextest.run_ignored.as_deref(), Some("all"));

    let mut configured_profile = nextest_profile();
    configured_profile.run_ignored = Some("only".to_owned());
    let mut plain_pkg = package();
    plain_pkg.name = "regular-crate".to_owned();
    let groups_cfg = derive_task_groups(&inputs(&plain_pkg, &configured_profile, &features))
        .expect("derivation ok");
    let nextest_cfg = groups_cfg
        .iter()
        .find(|g| g.kind == TaskKind::Nextest)
        .expect("has nextest");
    assert_eq!(nextest_cfg.run_ignored.as_deref(), Some("only"));
}
