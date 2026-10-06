//! F2 closure cases: identity attachment, rerun producer, shards, archives.
use std::collections::BTreeSet;

use velnor_actions_contract_planning::reverse_closure;
use velnor_actions_rust::tasks::{
    DigestSlot, ExtensionInputs, RustTaskIdentityExtension, parse_rerun_changed,
};
use velnor_actions_rust::{
    DeriveInputs, GroupExtensionInputs, TaskKind, adapter_entry_metadata, derive_task_groups,
    expand_shards_for_group,
};
use velnor_actions_rust_core::{
    CompileDriver, DepKind, Evidence, EvidenceStrength, LocalEdge, NextestProfile, PackageRecord,
    ProfileSource, RustExecutionProfile, TargetRecord, TestRunner, evidence_scan_excluded,
    local_edge_pairs,
};

/// Target entry for fixtures.
fn target(kind: &str, name: &str) -> TargetRecord {
    TargetRecord {
        kind: kind.to_owned(),
        name: name.to_owned(),
        test: true,
        doctest: kind == "lib",
        required_features: Vec::new(),
    }
}

/// Sample package with lib plus build script.
fn package() -> PackageRecord {
    PackageRecord {
        id: "a-id".to_owned(),
        name: "a".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: "crates/a/Cargo.toml".to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![
            target("lib", "a"),
            target("custom-build", "build-script-build"),
        ],
        features: vec!["default".to_owned()],
        has_build_script: true,
    }
}

/// Profile selecting Cargo and plain `cargo test`.
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

/// Profile selecting MBX and Nextest.
fn nextest_profile() -> RustExecutionProfile {
    RustExecutionProfile {
        compile_driver: CompileDriver::Mbx,
        test_runner: TestRunner::CargoNextest,
        evidence: Vec::new(),
        driver_source: ProfileSource::Detected,
        runner_source: ProfileSource::Detected,
        nextest_profile: NextestProfile::Ci,
        nextest_config: Some(".config/nextest.toml".to_owned()),
        run_ignored: None,
    }
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

/// Extension digests for `package` with `rerun` build inputs.
fn digests<'a>(
    package: &'a PackageRecord,
    targets: &'a [String],
    rerun: Option<&'a [String]>,
) -> GroupExtensionInputs<'a> {
    GroupExtensionInputs {
        package_id: &package.id,
        workspace_id: "workspace",
        profile: "default",
        manifest: &package.manifest,
        graph_digest: "graph",
        targets,
        config_digest: "config",
        lock_digest: DigestSlot::Known("lock".to_owned()),
        nextest_digest: DigestSlot::Known("nextest".to_owned()),
        archive_source: Some("stack/rust/crates/a/build/default"),
        rerun_inputs: rerun,
        has_build_script: package.has_build_script,
    }
}

/// Serialize one extension for stability comparisons.
fn json_of(ext: &RustTaskIdentityExtension) -> Option<serde_json::Value> {
    serde_json::to_value(ext).ok()
}

#[test]
fn warm_rerun_reuses_identical_extension() {
    let package = package();
    let profile = nextest_profile();
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&inputs(&package, &profile, &features)).expect("derive");
    let clippy = groups
        .iter()
        .find(|g| g.kind == TaskKind::Clippy)
        .expect("clippy");
    let targets = vec!["lib".to_owned()];
    let known: Vec<String> = vec!["proto/a.proto".to_owned()];
    let first = clippy.identity_extension(&digests(&package, &targets, Some(&known)));
    let second = clippy.identity_extension(&digests(&package, &targets, Some(&known)));
    assert_eq!(json_of(&first), json_of(&second));
    assert!(first.reuse_eligible().is_ok());
    assert!(second.coverage_eligible().is_ok());
    let other = vec!["proto/b.proto".to_owned()];
    let changed = clippy.identity_extension(&digests(&package, &targets, Some(&other)));
    assert_ne!(json_of(&first), json_of(&changed));
}

#[test]
fn declared_inputs_producer_attaches_rerun_inputs() {
    let output = "cargo::rerun-if-changed=proto/a.proto\ncargo:rerun-if-changed=schema/x.json\n";
    let rerun = parse_rerun_changed(output);
    assert_eq!(rerun.len(), 2);
    let package = package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&inputs(&package, &profile, &features)).expect("derive");
    let with = groups[1].clone().with_rerun_inputs(&rerun).expect("attach");
    assert_eq!(with.declared_inputs, rerun);
    let targets = vec!["lib".to_owned()];
    let ext = with.identity_extension(&digests(&package, &targets, Some(&rerun)));
    assert_eq!(ext.declared_inputs, rerun);
    assert!(ext.reuse_eligible().is_ok());
    let bad = vec!["../escape.proto".to_owned()];
    assert!(groups[1].clone().with_rerun_inputs(&bad).is_err());
}

#[test]
fn markdown_declared_but_never_evidence() {
    let package = package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&inputs(&package, &profile, &features)).expect("derive");
    let doc = vec!["docs/guide.md".to_owned()];
    let with = groups[1].clone().with_rerun_inputs(&doc).expect("attach");
    let targets = vec!["lib".to_owned()];
    let ext = with.identity_extension(&digests(&package, &targets, Some(&[])));
    assert_eq!(ext.declared_inputs, doc);
    assert!(evidence_scan_excluded("docs/guide.md"));
    assert!(evidence_scan_excluded("README.md"));
}

#[test]
fn groups_carry_extension_before_selection() {
    let package = package();
    let profile = nextest_profile();
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&inputs(&package, &profile, &features)).expect("derive");
    let targets = vec!["lib".to_owned()];
    let known: Vec<String> = Vec::new();
    let mut schemas = 0;
    for group in &groups {
        let ext = group.identity_extension(&digests(&package, &targets, Some(&known)));
        assert_eq!(ext.to_stack_extension().schema, "rust-task-identity-v1");
        assert_eq!(ext.package_id, package.id);
        assert_eq!(ext.kind, group.kind.as_str());
        assert_eq!(ext.driver, "mbx+cargo_nextest");
        assert!(ext.reuse_eligible().is_ok());
        schemas += 1;
    }
    assert!(schemas >= 4, "every group carries an extension");
}

#[test]
fn undeclared_reads_disable_reuse_and_coverage() {
    let package = package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&inputs(&package, &profile, &features)).expect("derive");
    let targets = vec!["lib".to_owned()];
    let unknown = groups[0].identity_extension(&digests(&package, &targets, None));
    assert!(unknown.undeclared_reads);
    assert!(unknown.reuse_eligible().is_err());
    assert!(unknown.coverage_eligible().is_err());
    assert!(unknown.conservative_execution_required());
    let known = vec!["proto/a.proto".to_owned()];
    let declared = groups[0].identity_extension(&digests(&package, &targets, Some(&known)));
    assert!(!declared.undeclared_reads);
    assert!(declared.reuse_eligible().is_ok());
    assert!(declared.coverage_eligible().is_ok());
    assert!(!declared.conservative_execution_required());
}

#[test]
fn closure_propagates_through_all_edge_kinds() {
    let edge = |from: &str, kind: DepKind, optional: bool, target: Option<&str>| LocalEdge {
        from: from.to_owned(),
        to: "a".to_owned(),
        kind,
        optional,
        target: target.map(str::to_owned),
    };
    let head = vec![
        edge("normal", DepKind::Normal, false, None),
        edge("build", DepKind::Build, false, None),
        edge("dev", DepKind::Dev, false, None),
        edge("optional", DepKind::Normal, true, None),
        edge("targeted", DepKind::Normal, false, Some("cfg(windows)")),
    ];
    let changed = BTreeSet::from(["a".to_owned()]);
    let selected = reverse_closure(&local_edge_pairs(&[]), &local_edge_pairs(&head), &changed);
    for consumer in ["a", "normal", "build", "dev", "optional", "targeted"] {
        assert!(selected.contains(consumer), "{consumer} must be selected");
    }
    let base = vec![edge("removed-opt", DepKind::Normal, true, None)];
    let selected = reverse_closure(&local_edge_pairs(&base), &local_edge_pairs(&[]), &changed);
    assert!(selected.contains("removed-opt"));
    assert!(selected.contains("a"));
}

#[test]
fn archive_trust_follows_source_build() {
    let targets = vec!["lib".to_owned()];
    let features = vec!["default".to_owned()];
    let declared: Vec<String> = Vec::new();
    let base = ExtensionInputs {
        package_id: "demo 0.1.0",
        workspace_id: "workspace",
        profile: "default",
        manifest: "Cargo.toml",
        graph_digest: "graph",
        targets: &targets,
        features: &features,
        target: "host",
        driver: CompileDriver::Cargo,
        runner: TestRunner::CargoNextest,
        config_digest: "config",
        lock_digest: DigestSlot::Known("lock".to_owned()),
        nextest_digest: DigestSlot::Known("nextest".to_owned()),
        kind: TaskKind::Build,
        archive_source: Some("stack/rust/root/build/default"),
        rerun_inputs: Some(&[]),
        has_build_script: false,
        declared_inputs: &declared,
    };
    let build = RustTaskIdentityExtension::for_task(&base);
    assert_eq!(
        build.archive_trust_source(),
        Some("stack/rust/root/build/default")
    );
    let mut cargo = base.clone();
    cargo.runner = TestRunner::CargoTest;
    assert!(
        RustTaskIdentityExtension::for_task(&cargo)
            .archive_trust_source()
            .is_none()
    );
    let mut test = base.clone();
    test.kind = TaskKind::Nextest;
    assert!(
        RustTaskIdentityExtension::for_task(&test)
            .archive_trust_source()
            .is_none()
    );
    let mut orphan = base;
    orphan.archive_source = None;
    assert!(
        RustTaskIdentityExtension::for_task(&orphan)
            .archive_trust_source()
            .is_none()
    );
}

#[test]
fn shard_expansion_is_nextest_only() {
    assert!(
        expand_shards_for_group(TaskKind::Nextest, TestRunner::CargoNextest, 2, false)
            .expect("expand")
    );
    assert!(
        !expand_shards_for_group(TaskKind::Nextest, TestRunner::CargoNextest, 1, false)
            .expect("single")
    );
    assert!(
        !expand_shards_for_group(TaskKind::Nextest, TestRunner::CargoNextest, 2, true)
            .expect("empty")
    );
    assert!(
        !expand_shards_for_group(TaskKind::Test, TestRunner::CargoNextest, 2, false)
            .expect("test-kind")
    );
    assert!(
        !expand_shards_for_group(TaskKind::Clippy, TestRunner::CargoTest, 1, false)
            .expect("clippy")
    );
    assert!(expand_shards_for_group(TaskKind::Test, TestRunner::CargoTest, 2, false).is_err());
    assert!(expand_shards_for_group(TaskKind::Nextest, TestRunner::CargoTest, 2, false).is_err());
}

#[test]
fn profile_runner_enters_task_identity() {
    let package = package();
    let features = vec!["default".to_owned()];
    let cargo = cargo_profile();
    let nextest = nextest_profile();
    let cargo_groups = derive_task_groups(&inputs(&package, &cargo, &features)).expect("derive");
    let nextest_groups =
        derive_task_groups(&inputs(&package, &nextest, &features)).expect("derive");
    let targets = vec!["lib".to_owned()];
    let known: Vec<String> = Vec::new();
    let cargo_ext = cargo_groups[0].identity_extension(&digests(&package, &targets, Some(&known)));
    let nextest_ext =
        nextest_groups[0].identity_extension(&digests(&package, &targets, Some(&known)));
    assert_eq!(cargo_ext.driver, "cargo+cargo_test");
    assert_eq!(nextest_ext.driver, "mbx+cargo_nextest");
    assert_ne!(json_of(&cargo_ext), json_of(&nextest_ext));
}

#[test]
fn adapter_entry_metadata_carries_driver_runner_evidence() {
    let package = package();
    let profile = cargo_profile();
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&inputs(&package, &profile, &features)).expect("derive");
    let sightings = vec![Evidence {
        path: "scripts/test.sh".to_owned(),
        line: 2,
        command_or_setting: "cargo test --package a".to_owned(),
        strength: EvidenceStrength::Durable,
    }];
    let meta = adapter_entry_metadata(&groups[1], &sightings);
    assert_eq!(
        meta["package_id"],
        serde_json::Value::String("a-id".to_owned())
    );
    assert_eq!(meta["kind"], serde_json::Value::String("test".to_owned()));
    assert_eq!(
        meta["compile_driver"],
        serde_json::Value::String("cargo".to_owned())
    );
    assert_eq!(
        meta["test_runner"],
        serde_json::Value::String("cargo_test".to_owned())
    );
    let ids = meta["evidence_ids"].as_array().expect("evidence ids");
    assert_eq!(ids.len(), 1);
    assert!(
        ids[0]
            .as_str()
            .is_some_and(|id| id.starts_with("scripts/test.sh:2:"))
    );
}
