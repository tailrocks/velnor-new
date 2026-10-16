//! Contract tests for `[agents]`, `[[stacks.rust.suites]]`, `[workflow.dispatch]`,
//! `workflow.preserve_workflows`, and tightened `ScheduleTrigger::validate`.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    AgentsConfig, ClaudeMdMode, ContractError, NextestRunIgnored, RustStackConfig,
    RustSuiteArtifact, RustSuiteArtifactWhen, RustSuiteBuildMode, RustSuiteConfig,
    RustSuiteTrigger, ScheduleTrigger, WorkflowDispatchConfig, WorkflowDispatchInputConfig,
    WorkflowDispatchInputType,
};

use crate::impl_remed_contract::valid_config;

const FILE: &str = ".velnor/config.toml";

fn expect_config_err(
    result: Result<(), ContractError>,
    expected_key: &str,
    expected_problem: &str,
) {
    let Err(ContractError::Config {
        key_path, problem, ..
    }) = result
    else {
        return;
    };
    assert_eq!(key_path, expected_key, "problem was {problem}");
    assert!(
        problem.contains(expected_problem),
        "expected {expected_problem} in {problem}"
    );
}

#[test]
fn agents_config_defaults_to_import_and_roundtrips_symlink() {
    let default = AgentsConfig::default();
    assert_eq!(default.claude_md, ClaudeMdMode::Import);
    assert!(default.is_default());
    assert!(default.claude_md.is_default());
    assert_eq!(default.claude_md.as_str(), "import");

    let cfg = valid_config();
    let json = serde_json::to_value(&cfg).unwrap_or_default();
    assert!(json.get("agents").is_none(), "default agents omitted");

    let mut symlink_cfg = valid_config();
    symlink_cfg.agents.claude_md = ClaudeMdMode::Symlink;
    assert!(!symlink_cfg.agents.is_default());
    assert_eq!(symlink_cfg.agents.claude_md.as_str(), "symlink");
    assert_eq!(symlink_cfg.validate(FILE), Ok(()));
    let serialized = serde_json::to_value(&symlink_cfg).unwrap_or_default();
    assert_eq!(
        serialized
            .get("agents")
            .and_then(|v| v.get("claude_md"))
            .and_then(serde_json::Value::as_str),
        Some("symlink")
    );
}

#[test]
fn schedule_trigger_validates_numeric_bounds_and_ranges() {
    for valid in [
        "0 2 * * *",
        "0 6 * * 1",
        "*/15 0-23/2 1,15 1-12 0-7",
        "59 23 31 12 7",
        "0 0 1 1 0",
    ] {
        assert_eq!(ScheduleTrigger::single(valid).validate(), Ok(()));
    }
    for invalid in [
        "",
        "0 2 * *",
        "0 2 * * * *",
        "60 0 * * *",
        "0 24 * * *",
        "0 2 0 * *",
        "0 2 32 13 8",
        "0 2 * 0 *",
        "0 2 * 13 *",
        "0 2 * * 8",
        "*/0 * * * *",
        "10-5 * * * *",
        "1,,2 * * * *",
        "a b c d e",
    ] {
        assert!(ScheduleTrigger::single(invalid).validate().is_err());
    }
}

#[test]
fn valid_rust_suites_and_dispatch_pass_validation() {
    let mut cfg = valid_config();
    cfg.workflow.dispatch = WorkflowDispatchConfig {
        inputs: BTreeMap::from([
            (
                "suite_group".to_owned(),
                WorkflowDispatchInputConfig {
                    kind: WorkflowDispatchInputType::Choice,
                    description: Some("Suite group to execute".to_owned()),
                    required: false,
                    default: Some("fast".to_owned()),
                    options: vec!["fast".to_owned(), "extended".to_owned()],
                },
            ),
            (
                "test_filter".to_owned(),
                WorkflowDispatchInputConfig {
                    kind: WorkflowDispatchInputType::String,
                    description: None,
                    required: false,
                    default: Some(String::new()),
                    options: Vec::new(),
                },
            ),
        ]),
    };
    cfg.workflow.preserve_workflows = vec![".github/workflows/custom-check.yml".to_owned()];

    let mut fast = RustSuiteConfig::new("fast-suite", "fast");
    fast.run_ignored = Some(NextestRunIgnored::Only);
    fast.partitions = 2;
    fast.build = RustSuiteBuildMode::Archive;
    fast.timeout_minutes = 30;
    fast.test_threads = Some(4);
    fast.runner_label = Some("ubuntu-24.04".to_owned());
    fast.required = true;
    fast.artifacts = vec![RustSuiteArtifact {
        name: "fast-artifacts-{shard}".to_owned(),
        paths: vec!["target/suite-output/diff".to_owned()],
        when: RustSuiteArtifactWhen::Failure,
        retention_days: 14,
    }];

    let mut nightly = RustSuiteConfig::new("nightly-suite", "extended");
    nightly.run_ignored = Some(NextestRunIgnored::All);
    nightly.partitions = 12;
    nightly.build = RustSuiteBuildMode::Archive;
    nightly.timeout_minutes = 60;
    nightly.workflow = Some("nightly-suites".to_owned());
    nightly.triggers = vec![
        RustSuiteTrigger::Schedule,
        RustSuiteTrigger::WorkflowDispatch,
    ];
    nightly.schedule = vec!["0 2 * * *".to_owned()];
    nightly.cancel_in_progress = Some(false);
    nightly.allow_failure = true;
    nightly.filter = Some("test(suite_)".to_owned());
    nightly.filter_input = Some("test_filter".to_owned());
    nightly.dispatch_inputs =
        BTreeMap::from([("suite_group".to_owned(), vec!["extended".to_owned()])]);
    nightly.package = Some("demo-pkg".to_owned());
    nightly.manifest_path = Some("Cargo.toml".to_owned());
    nightly.env = BTreeMap::from([("SUITE_MODE".to_owned(), "1".to_owned())]);
    nightly.artifacts = vec![RustSuiteArtifact {
        name: "nightly-{partition}".to_owned(),
        paths: vec!["target/suite-output/report.html".to_owned()],
        when: RustSuiteArtifactWhen::Always,
        retention_days: 30,
    }];

    let mut rust = RustStackConfig::default_config();
    rust.suites = vec![fast, nightly];
    cfg.stacks.rust = Some(rust);

    assert_eq!(cfg.validate(FILE), Ok(()));
}

#[test]
fn rust_suite_id_profile_and_scalar_bounds_fail_closed() {
    type Case = (fn(&mut RustSuiteConfig), &'static str, &'static str);
    let cases: &[Case] = &[
        (|s| s.id = String::new(), "id", "bad_suite_id"),
        (|s| s.id = "-bad".to_owned(), "id", "bad_suite_id"),
        (|s| s.id = "bad-".to_owned(), "id", "bad_suite_id"),
        (|s| s.id = "Bad_Id".to_owned(), "id", "bad_suite_id"),
        (|s| s.id = "required".to_owned(), "id", "reserved_suite_id"),
        (|s| s.id = "plan".to_owned(), "id", "reserved_suite_id"),
        (
            |s| s.profile = "-flag".to_owned(),
            "profile",
            "bad_suite_profile",
        ),
        (|s| s.partitions = 0, "partitions", "bad_suite_partitions"),
        (|s| s.partitions = 65, "partitions", "bad_suite_partitions"),
        (
            |s| s.timeout_minutes = 0,
            "timeout_minutes",
            "bad_suite_timeout",
        ),
        (
            |s| s.timeout_minutes = 361,
            "timeout_minutes",
            "bad_suite_timeout",
        ),
        (
            |s| s.test_threads = Some(0),
            "test_threads",
            "bad_suite_test_threads",
        ),
        (
            |s| s.test_threads = Some(65),
            "test_threads",
            "bad_suite_test_threads",
        ),
        (
            |s| s.runner_label = Some("ubuntu-latest".to_owned()),
            "runner_label",
            "unsupported_label",
        ),
        (
            |s| s.workflow = Some("ci".to_owned()),
            "workflow",
            "reserved_suite_workflow",
        ),
        (
            |s| s.workflow = Some("bad.yml".to_owned()),
            "workflow",
            "bad_suite_workflow",
        ),
    ];
    for &(mutate, field, problem) in cases {
        check_suite_err(mutate, field, problem);
    }
}

fn check_suite_err(mutate: fn(&mut RustSuiteConfig), field: &str, problem: &str) {
    let mut stack = RustStackConfig::default_config();
    let mut suite = RustSuiteConfig::new("valid-suite", "default");
    mutate(&mut suite);
    stack.suites = vec![suite];
    let key = format!("stacks.rust.suites[0].{field}");
    let res = stack.validate(FILE);
    assert!(res.is_err(), "expected error for {key}: {problem}");
    expect_config_err(res, &key, problem);
}

#[test]
fn rust_suite_triggers_and_schedule_fail_closed() {
    check_suite_err(|s| s.triggers.clear(), "triggers", "empty_suite_triggers");
    check_suite_err(
        |s| s.triggers = vec![RustSuiteTrigger::Push, RustSuiteTrigger::Push],
        "triggers",
        "duplicate_suite_trigger",
    );
    check_suite_err(
        |s| s.triggers = vec![RustSuiteTrigger::Schedule],
        "schedule",
        "empty_suite_schedule",
    );
    check_suite_err(
        |s| {
            s.triggers = vec![RustSuiteTrigger::Schedule];
            s.schedule = vec!["99 25 * * *".to_owned()];
        },
        "schedule",
        "bad_cron",
    );
    check_suite_err(
        |s| s.schedule = vec!["0 2 * * *".to_owned()],
        "schedule",
        "schedule_requires_schedule_trigger",
    );
    check_suite_err(
        |s| {
            s.triggers = vec![RustSuiteTrigger::WorkflowDispatch];
            s.required = true;
        },
        "required",
        "required_suite_needs_pr_or_push_trigger",
    );
    check_suite_err(
        |s| {
            s.allow_failure = true;
            s.required = true;
        },
        "allow_failure",
        "allow_failure_cannot_be_required",
    );
    check_suite_err(
        |s| s.manifest_path = Some("../Cargo.toml".to_owned()),
        "manifest_path",
        "bad_suite_manifest_path",
    );

    let mut stack = RustStackConfig::default_config();
    let mut s1 = RustSuiteConfig::new("suite-a", "default");
    s1.cancel_in_progress = Some(true);
    let mut s2 = RustSuiteConfig::new("suite-b", "default");
    s2.cancel_in_progress = Some(false);
    stack.suites = vec![s1, s2];
    expect_config_err(
        stack.validate(FILE),
        "stacks.rust.suites[1].cancel_in_progress",
        "conflicting_suite_cancel_in_progress",
    );
}

#[test]
fn rust_suite_artifacts_fail_closed() {
    let cases = [
        ("bad-{u}", "target/out", 14, "name", "bad_artifact_name"),
        ("good", "../escape", 14, "paths", "bad_artifact_path"),
        (
            "good",
            "target/out",
            91,
            "retention_days",
            "bad_artifact_retention_days",
        ),
    ];
    for (name, path, retention, field, problem) in cases {
        let mut stack = RustStackConfig::default_config();
        let mut suite = RustSuiteConfig::new("valid-suite", "default");
        suite.artifacts = vec![RustSuiteArtifact {
            name: name.to_owned(),
            paths: vec![path.to_owned()],
            when: RustSuiteArtifactWhen::Failure,
            retention_days: retention,
        }];
        stack.suites = vec![suite];
        let key = format!("stacks.rust.suites[0].artifacts[0].{field}");
        expect_config_err(stack.validate(FILE), &key, problem);
    }
}

#[test]
fn dispatch_cross_validation_and_preserve_workflows_fail_closed() {
    let mut cfg = valid_config();
    for path in [".github/workflows/ci.yml", ".github/workflows/suites.yml"] {
        cfg.workflow.preserve_workflows = vec![path.to_owned()];
        expect_config_err(
            cfg.validate(FILE),
            "workflow.preserve_workflows",
            "cannot_preserve_generator_workflow",
        );
    }

    cfg.workflow.preserve_workflows = vec![".github/workflows/nightly.yml".to_owned()];
    let mut custom_wf_suite = RustSuiteConfig::new("nightly-suite", "default");
    custom_wf_suite.workflow = Some("nightly".to_owned());
    let mut rust = RustStackConfig::default_config();
    rust.suites = vec![custom_wf_suite];
    cfg.stacks.rust = Some(rust);
    expect_config_err(
        cfg.validate(FILE),
        "workflow.preserve_workflows",
        "preserve_workflow_generator_owned:.github/workflows/nightly.yml",
    );

    cfg.workflow.preserve_workflows.clear();
    cfg.workflow.dispatch = WorkflowDispatchConfig {
        inputs: BTreeMap::from([(
            "group".to_owned(),
            WorkflowDispatchInputConfig {
                kind: WorkflowDispatchInputType::Choice,
                description: None,
                required: false,
                default: Some("fast".to_owned()),
                options: vec!["fast".to_owned()],
            },
        )]),
    };

    let mut suite = RustSuiteConfig::new("manual-suite", "default");
    suite.triggers = vec![RustSuiteTrigger::WorkflowDispatch];
    suite.dispatch_inputs = BTreeMap::from([("group".to_owned(), vec!["missing".to_owned()])]);
    let mut rust = RustStackConfig::default_config();
    rust.suites = vec![suite];
    cfg.stacks.rust = Some(rust);

    expect_config_err(
        cfg.validate(FILE),
        "stacks.rust.suites[0].dispatch_inputs.group",
        "unknown_dispatch_option:missing",
    );
}
