// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/obligation_order.rs"]
mod obligation_order;

use std::error::Error;
use std::ffi::OsString;
use std::io;

use self::obligation_order::obligation_order_key;
use velnor_actions_rust::tasks::{
    DeriveInputs, TaskGroup, cargo_payload_argv, cargo_payload_with_profile, derive_task_groups,
    require_nextest_for_shards, shard_task_id,
};
use velnor_actions_rust::{
    CompileDriver, NextestProfile, PackageRecord, ProfileSource, RustExecutionProfile,
    TargetRecord, TaskKind, TestRunner, VelnorV1TaskOwner, cargo_payload_env,
    concurrent_target_lane, payload_env_for_kind, runs_before_parallel_checks, task_kind_rank,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

fn fixture_package(manifest: &str) -> PackageRecord {
    PackageRecord {
        id: "demo-id".to_owned(),
        name: "demo".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: manifest.to_owned(),
        external: false,
        in_workspace: true,
        targets: vec![
            TargetRecord {
                kind: "lib".to_owned(),
                name: "demo".to_owned(),
                test: true,
                doctest: true,
                required_features: Vec::new(),
            },
            TargetRecord {
                kind: "test".to_owned(),
                name: "integ".to_owned(),
                test: true,
                doctest: false,
                required_features: Vec::new(),
            },
        ],
        features: Vec::new(),
        has_build_script: false,
        v1_task_owner: VelnorV1TaskOwner::default(),
    }
}

fn fixture_profile() -> RustExecutionProfile {
    RustExecutionProfile {
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoNextest,
        evidence: Vec::new(),
        driver_source: ProfileSource::Declared,
        runner_source: ProfileSource::Declared,
        nextest_profile: NextestProfile::Ci,
        nextest_config: None,
        run_ignored: None,
    }
}

fn fixture_features() -> Vec<String> {
    vec!["serde".to_owned(), "tokio".to_owned()]
}

fn contains(argv: &[OsString], flag: &str) -> bool {
    argv.iter().any(|arg| arg == flag)
}

fn check_lane_classification() -> Result<(), Box<dyn Error>> {
    if !runs_before_parallel_checks("build") {
        return Err(fail("build must run before parallel checks"));
    }
    for kind in ["clippy", "test", "nextest", "doctest", "doc", "fmt"] {
        if runs_before_parallel_checks(kind) {
            return Err(fail("only build runs before parallel checks"));
        }
    }
    if concurrent_target_lane("clippy") != Some("velnor-clippy") {
        return Err(fail("clippy lane drifted"));
    }
    if concurrent_target_lane("doc") != Some("velnor-doc") {
        return Err(fail("doc lane drifted"));
    }
    for kind in ["build", "test", "nextest", "doctest", "fmt"] {
        if concurrent_target_lane(kind).is_some() {
            return Err(fail("build/test lane must stay on the shared target dir"));
        }
    }
    if concurrent_target_lane("clippy") == concurrent_target_lane("doc") {
        return Err(fail("clippy and doc lanes collide"));
    }
    Ok(())
}

fn check_serialized_order() -> Result<(), Box<dyn Error>> {
    let key = |kind: &str| obligation_order_key("rust", kind, "stack/rust/demo");
    if key("fmt") >= key("clippy") {
        return Err(fail("fmt must serialize before clippy"));
    }
    if key("clippy") >= key("build") {
        return Err(fail("clippy must serialize before build"));
    }
    if key("build") >= key("test") {
        return Err(fail("build must serialize before test"));
    }
    if key("test") != key("nextest") {
        return Err(fail("test and nextest share one serialization slot"));
    }
    if key("nextest") >= key("doctest") {
        return Err(fail("test must serialize before doctests"));
    }
    if key("doctest") >= key("doc") {
        return Err(fail("doctests must serialize before doc"));
    }
    Ok(())
}

fn check_group_plan() -> Result<(), Box<dyn Error>> {
    let package = fixture_package("crates/demo/Cargo.toml");
    let profile = fixture_profile();
    let features = fixture_features();
    let groups = derive_task_groups(&DeriveInputs {
        package: &package,
        profile: &profile,
        configuration: "default",
        features: &features,
        all_features: false,
        clippy_all_features: false,
        target: "x86_64-unknown-linux-gnu",
        explicit_fmt: true,
    })
    .map_err(|err| fail(&format!("derive failed: {err}")))?;
    let kinds: Vec<TaskKind> = groups.iter().map(|group| group.kind).collect();
    if kinds
        != [
            TaskKind::Clippy,
            TaskKind::Build,
            TaskKind::Nextest,
            TaskKind::Doctest,
            TaskKind::Doc,
            TaskKind::Fmt,
        ]
    {
        return Err(fail("group lane plan drifted"));
    }
    let mut ids: Vec<&str> = groups.iter().map(|group| group.task_id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != groups.len() {
        return Err(fail("each unit must keep its own result identity"));
    }
    for group in &groups {
        if !group.task_id.contains(group.kind.as_str()) {
            return Err(fail("unit identity lost its command family"));
        }
    }
    let build_id = &groups[1].task_id;
    let nextest = &groups[2];
    if nextest.depends_on != vec![build_id.clone()] {
        return Err(fail("test unit must keep its build gate"));
    }
    for group in [&groups[0], &groups[3], &groups[4], &groups[5]] {
        if !group.depends_on.is_empty() || !group.gated_by.is_empty() {
            return Err(fail("independent check gated on the unit lane"));
        }
    }
    if nextest.no_test_targets {
        return Err(fail("test targets misclassified as absent"));
    }
    if groups[3].no_test_targets {
        return Err(fail("doctest targets misclassified as absent"));
    }
    Ok(())
}

fn check_exact_commands_preserved() -> Result<(), Box<dyn Error>> {
    let package = fixture_package("crates/demo/Cargo.toml");
    let profile = fixture_profile();
    let features = fixture_features();
    let inputs = DeriveInputs {
        package: &package,
        profile: &profile,
        configuration: "default",
        features: &features,
        all_features: false,
        clippy_all_features: false,
        target: "x86_64-unknown-linux-gnu",
        explicit_fmt: true,
    };
    let groups =
        derive_task_groups(&inputs).map_err(|err| fail(&format!("derive failed: {err}")))?;
    for group in &groups {
        let first =
            cargo_payload_argv(group).map_err(|err| fail(&format!("argv failed: {err}")))?;
        let second =
            cargo_payload_argv(group).map_err(|err| fail(&format!("argv failed: {err}")))?;
        if first != second {
            return Err(fail("exact package command not deterministic"));
        }
        if group.kind == TaskKind::Fmt {
            continue;
        }
        for flag in ["--features", "--target", "--manifest-path", "--package"] {
            if !contains(&first, flag) {
                return Err(fail("command lost a preserved package input"));
            }
        }
    }
    check_kind_argv_preserved(&groups)?;
    check_warning_flags_preserved()?;
    Ok(())
}

fn check_kind_argv_preserved(groups: &[TaskGroup]) -> Result<(), Box<dyn Error>> {
    let clippy =
        cargo_payload_argv(&groups[0]).map_err(|err| fail(&format!("argv failed: {err}")))?;
    if !contains(&clippy, "--all-targets") {
        return Err(fail("clippy lost its target scope"));
    }
    let tail = clippy
        .iter()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .cloned()
        .collect::<Vec<_>>();
    if tail
        != [
            OsString::from("--"),
            OsString::from("-D"),
            OsString::from("warnings"),
        ]
    {
        return Err(fail("clippy lost its warning flag"));
    }
    let nextest = cargo_payload_with_profile(&groups[2])
        .map_err(|err| fail(&format!("argv failed: {err}")))?;
    for flag in ["--profile", "ci", "--no-tests", "fail"] {
        if !contains(&nextest, flag) {
            return Err(fail("nextest lost its profile or execution contract"));
        }
    }
    if contains(&nextest, "--run-ignored") {
        return Err(fail("nextest invented a run-ignored mode"));
    }
    let build = cargo_payload_with_profile(&groups[1])
        .map_err(|err| fail(&format!("argv failed: {err}")))?;
    for flag in ["binaries-only", "--profile", "ci"] {
        if !contains(&build, flag) {
            return Err(fail("build lost its nextest collection profile"));
        }
    }
    let doctest =
        cargo_payload_argv(&groups[3]).map_err(|err| fail(&format!("argv failed: {err}")))?;
    if !contains(&doctest, "--doc") {
        return Err(fail("doctest lost its doc scope"));
    }
    let doc = cargo_payload_argv(&groups[4]).map_err(|err| fail(&format!("argv failed: {err}")))?;
    if !contains(&doc, "--no-deps") {
        return Err(fail("doc lost its dependency scope"));
    }
    let fmt = cargo_payload_argv(&groups[5]).map_err(|err| fail(&format!("argv failed: {err}")))?;
    if !contains(&fmt, "--check") {
        return Err(fail("fmt lost its check mode"));
    }
    Ok(())
}

fn check_warning_flags_preserved() -> Result<(), Box<dyn Error>> {
    let doc_env = cargo_payload_env(TaskKind::Doc);
    if !doc_env
        .iter()
        .any(|(key, value)| key == "RUSTDOCFLAGS" && value == velnor_actions_rust::DENY_WARNINGS)
    {
        return Err(fail("doc lost its warning flag"));
    }
    if !payload_env_for_kind("doc")
        .iter()
        .any(|(key, _)| key == "RUSTDOCFLAGS")
    {
        return Err(fail("doc payload lost its warning environment"));
    }
    for kind in ["clippy", "build", "test", "nextest", "doctest", "fmt"] {
        if payload_env_for_kind(kind)
            .iter()
            .any(|(key, _)| key == "RUSTDOCFLAGS")
        {
            return Err(fail("warning environment leaked outside the doc lane"));
        }
    }
    Ok(())
}

#[path = "gc1_t33_a.rs"]
mod gc1_t33_a;
