//! P03 extension matrix: input flips, resolution states, path rules.
//!
//! Unregistered: the parent wires this module into `velnor_rust.rs`
//! (P09 pattern). Cases run through the public adapter API only.

use velnor_actions_rust::identity::{UnresolvedInput, normalize_identity_path, unresolved_inputs};
use velnor_actions_rust::tasks::{DigestSlot, RustTaskIdentityExtension, parse_rerun_changed};
use velnor_actions_rust::{
    CompileDriver, DeriveInputs, GroupExtensionInputs, PackageRecord, ProfileSource,
    RustExecutionProfile, TargetRecord, TaskGroup, TaskKind, TestRunner, derive_task_groups,
};

/// Test outcome boxing every error type.
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Known digest slot over a cloned digest.
fn known(digest: &str) -> DigestSlot {
    DigestSlot::Known(digest.to_owned())
}

/// Unknown digest slot.
fn unknown() -> DigestSlot {
    DigestSlot::Unknown("unprobed".to_owned())
}

/// Package fixture with lib target and no build script.
fn package() -> PackageRecord {
    PackageRecord {
        id: "a-id".to_owned(),
        name: "a".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: "crates/a/Cargo.toml".to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![TargetRecord {
            kind: "lib".to_owned(),
            name: "a".to_owned(),
            test: true,
            doctest: true,
            required_features: Vec::new(),
        }],
        features: vec!["default".to_owned()],
        has_build_script: false,
    }
}

/// Cargo-test profile fixture.
fn profile() -> RustExecutionProfile {
    RustExecutionProfile {
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        evidence: Vec::new(),
        driver_source: ProfileSource::Detected,
        runner_source: ProfileSource::Detected,
        nextest_profile: velnor_actions_rust::NextestProfile::Default,
        nextest_config: None,
        run_ignored: None,
    }
}

/// First clippy group derived for the fixture package.
fn clippy() -> TestResult<TaskGroup> {
    let package = package();
    let profile = profile();
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&DeriveInputs {
        package: &package,
        profile: &profile,
        configuration: "default",
        features: &features,
        target: "host",
        explicit_fmt: false,
    })?;
    let Some(group) = groups
        .into_iter()
        .find(|group| group.kind == TaskKind::Clippy)
    else {
        return Err(std::io::Error::other("clippy group").into());
    };
    Ok(group)
}

/// Extension inputs over explicit digests.
fn inputs<'a>(
    graph: &'a str,
    config: &'a str,
    lock: DigestSlot,
    targets: &'a [String],
) -> GroupExtensionInputs<'a> {
    GroupExtensionInputs {
        package_id: "a-id",
        workspace_id: "workspace",
        profile: "default",
        manifest: "crates/a/Cargo.toml",
        graph_digest: graph,
        targets,
        config_digest: config,
        lock_digest: lock,
        nextest_digest: unknown(),
        archive_source: None,
        rerun_inputs: Some(&[]),
        has_build_script: false,
    }
}

/// Serialized extension data for comparisons.
fn data_of(ext: &RustTaskIdentityExtension) -> TestResult<serde_json::Value> {
    Ok(serde_json::to_value(ext.to_stack_extension().data)?)
}

#[test]
fn every_semantic_input_flips_the_extension() -> TestResult {
    let group = clippy()?;
    let targets = vec!["lib:a".to_owned()];
    let graph_a = velnor_actions_contract::digest_b3(b"graph-a");
    let graph_b = velnor_actions_contract::digest_b3(b"graph-b");
    let config = velnor_actions_contract::digest_b3(b"config");
    let lock_a = velnor_actions_contract::digest_b3(b"lock-a");
    let lock_b = velnor_actions_contract::digest_b3(b"lock-b");
    let base =
        data_of(&group.identity_extension(&inputs(&graph_a, &config, known(&lock_a), &targets)))?;
    assert_eq!(
        base,
        data_of(&group.identity_extension(&inputs(&graph_a, &config, known(&lock_a), &targets)))?
    );
    assert_ne!(
        base,
        data_of(&group.identity_extension(&inputs(&graph_b, &config, known(&lock_a), &targets)))?
    );
    assert_ne!(
        base,
        data_of(&group.identity_extension(&inputs(&graph_a, &config, known(&lock_b), &targets)))?
    );
    assert_ne!(
        base,
        data_of(&group.identity_extension(&inputs(&graph_a, &config, unknown(), &targets)))?
    );
    Ok(())
}

#[test]
fn declared_inputs_and_profile_flip_the_extension() -> TestResult {
    let group = clippy()?;
    let targets = vec!["lib:a".to_owned()];
    let graph_a = velnor_actions_contract::digest_b3(b"graph-a");
    let config = velnor_actions_contract::digest_b3(b"config");
    let lock_a = velnor_actions_contract::digest_b3(b"lock-a");
    let base =
        data_of(&group.identity_extension(&inputs(&graph_a, &config, known(&lock_a), &targets)))?;
    let rerun_a = parse_rerun_changed("cargo::rerun-if-changed=proto/a.proto\n");
    let rerun_b = parse_rerun_changed("cargo::rerun-if-changed=proto/b.proto\n");
    let one = group.clone().with_rerun_inputs(&rerun_a)?;
    let two = group.clone().with_rerun_inputs(&rerun_b)?;
    assert_ne!(
        data_of(&one.identity_extension(&inputs(&graph_a, &config, known(&lock_a), &targets)))?,
        data_of(&two.identity_extension(&inputs(&graph_a, &config, known(&lock_a), &targets)))?
    );
    let mut featured = group.clone();
    featured.features = vec!["serde".to_owned()];
    let featured_data = data_of(&featured.identity_extension(&inputs(
        &graph_a,
        &config,
        known(&lock_a),
        &targets,
    )))?;
    assert_ne!(base, featured_data);
    let profiled = GroupExtensionInputs {
        profile: "ci",
        ..inputs(&graph_a, &config, known(&lock_a), &targets)
    };
    assert_ne!(base, data_of(&group.identity_extension(&profiled))?);
    let nextest = velnor_actions_contract::digest_b3(b"nextest");
    let with_nextest = GroupExtensionInputs {
        nextest_digest: known(&nextest),
        ..inputs(&graph_a, &config, known(&lock_a), &targets)
    };
    assert_ne!(base, data_of(&group.identity_extension(&with_nextest))?);
    Ok(())
}

#[test]
fn relocated_checkout_keeps_extension_identity() -> TestResult {
    let group = clippy()?;
    let targets = vec!["lib:a".to_owned()];
    let graph = velnor_actions_contract::digest_b3(b"graph");
    let config = velnor_actions_contract::digest_b3(b"config");
    let lock = velnor_actions_contract::digest_b3(b"lock");
    let ext = group.identity_extension(&inputs(&graph, &config, known(&lock), &targets));
    assert_eq!(ext.manifest, "crates/a/Cargo.toml");
    assert_eq!(
        data_of(&ext)?,
        data_of(&group.identity_extension(&inputs(&graph, &config, known(&lock), &targets)))?
    );
    assert!(unresolved_inputs(&ext).is_empty());
    Ok(())
}

#[test]
fn unknown_inputs_stay_explicit_never_absent() -> TestResult {
    let group = clippy()?;
    let targets = Vec::new();
    let graph = velnor_actions_contract::digest_b3(b"graph");
    let config = velnor_actions_contract::digest_b3(b"config");
    let ext = group.identity_extension(&inputs(&graph, &config, unknown(), &targets));
    assert_eq!(unresolved_inputs(&ext), vec![UnresolvedInput::Lockfile]);
    let data = data_of(&ext)?;
    assert!(
        data.get("lock_digest")
            .is_some_and(serde_json::Value::is_null)
    );
    Ok(())
}

#[test]
fn identity_paths_preserve_case_unicode_and_reject_bad() -> TestResult {
    assert_eq!(
        normalize_identity_path("Crates/Äpfel/x.proto").expect("unicode"),
        "Crates/Äpfel/x.proto"
    );
    for bad in ["", "/abs", "a/../b", "a\\b", "a\0b"] {
        assert!(normalize_identity_path(bad).is_err(), "{bad:?}");
    }
    let group = clippy()?;
    let targets = Vec::new();
    let graph = velnor_actions_contract::digest_b3(b"graph");
    let config = velnor_actions_contract::digest_b3(b"config");
    assert!(
        group
            .identity_extension_verified(&inputs(&graph, &config, unknown(), &targets))
            .is_ok()
    );
    let hostile = TaskGroup {
        declared_inputs: vec!["../escape".to_owned()],
        ..group.clone()
    };
    assert!(
        hostile
            .identity_extension_verified(&inputs(&graph, &config, unknown(), &targets))
            .is_err()
    );
    Ok(())
}
