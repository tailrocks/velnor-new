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
    let valid = "0 2 * * *|0 6 * * 1|*/15 0-23/2 1,15 1-12 0-7|59 23 31 12 7|0 0 1 1 0";
    for cron in valid.split('|') {
        assert_eq!(ScheduleTrigger::single(cron).validate(), Ok(()));
    }
    let invalid = "|0 2 * *|0 2 * * * *|60 0 * * *|0 24 * * *|0 2 0 * *|0 2 32 13 8|0 2 * 0 *|0 2 * 13 *|0 2 * * 8|*/0 * * * *|10-5 * * * *|1,,2 * * * *|a b c d e";
    for cron in invalid.split('|') {
        assert!(ScheduleTrigger::single(cron).validate().is_err());
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
    chk(|s| s.id = String::new(), "id", "bad_suite_id");
    chk(|s| s.id = "-bad".into(), "id", "bad_suite_id");
    chk(|s| s.id = "bad-".into(), "id", "bad_suite_id");
    chk(|s| s.id = "Bad_Id".into(), "id", "bad_suite_id");
    chk(|s| s.id = "required".into(), "id", "reserved_suite_id");
    chk(|s| s.id = "plan".into(), "id", "reserved_suite_id");
    chk(|s| s.profile = "-f".into(), "profile", "bad_suite_profile");
    chk(|s| s.partitions = 0, "partitions", "bad_suite_partitions");
    chk(|s| s.partitions = 65, "partitions", "bad_suite_partitions");
    chk(
        |s| s.timeout_minutes = 0,
        "timeout_minutes",
        "bad_suite_timeout",
    );
    chk(
        |s| s.timeout_minutes = 361,
        "timeout_minutes",
        "bad_suite_timeout",
    );
    chk(
        |s| s.test_threads = Some(0),
        "test_threads",
        "bad_suite_test_threads",
    );
    chk(
        |s| s.test_threads = Some(65),
        "test_threads",
        "bad_suite_test_threads",
    );
    chk(
        |s| s.runner_label = Some("bad".into()),
        "runner_label",
        "unsupported_label",
    );
    chk(
        |s| s.workflow = Some("ci".into()),
        "workflow",
        "reserved_suite_workflow",
    );
    chk(
        |s| s.workflow = Some("bad.yml".into()),
        "workflow",
        "bad_suite_workflow",
    );
}

fn chk(mutate: fn(&mut RustSuiteConfig), field: &str, problem: &str) {
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
    use RustSuiteTrigger::{PullRequest, Push, Schedule, WorkflowDispatch};
    chk(|s| s.triggers.clear(), "triggers", "empty_suite_triggers");
    chk(
        |s| s.triggers = vec![Push, Push],
        "triggers",
        "duplicate_suite_trigger",
    );
    chk(
        |s| s.triggers = vec![Schedule],
        "schedule",
        "empty_suite_schedule",
    );
    chk(
        |s| {
            s.triggers = vec![Schedule];
            s.schedule = vec!["99 25 * * *".into()];
        },
        "schedule",
        "bad_cron",
    );
    chk(
        |s| s.schedule = vec!["0 2 * * *".into()],
        "schedule",
        "schedule_requires_schedule_trigger",
    );
    chk(
        |s| {
            s.triggers = vec![Schedule];
            s.schedule = vec!["0 2 * * *".into()];
            s.cancel_in_progress = Some(true);
        },
        "cancel_in_progress",
        "schedule_suite_cannot_cancel_in_progress",
    );
    for triggers in [vec![WorkflowDispatch], vec![PullRequest], vec![Push]] {
        let mut stack = RustStackConfig::default_config();
        let mut suite = RustSuiteConfig::new("valid-suite", "default");
        suite.triggers = triggers;
        suite.required = true;
        stack.suites = vec![suite];
        expect_config_err(
            stack.validate(FILE),
            "stacks.rust.suites[0].required",
            "required_suite_requires_pr_and_push_triggers",
        );
    }
    chk(
        |s| {
            s.allow_failure = true;
            s.required = true;
        },
        "allow_failure",
        "allow_failure_cannot_be_required",
    );
    chk(
        |s| s.manifest_path = Some("../Cargo.toml".into()),
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
fn rust_suite_artifacts_and_reserved_env_fail_closed() {
    let cases = [
        ("bad-{u}", "target/out", 14, "name", "bad_artifact_name"),
        ("good", "../escape", 14, "paths", "bad_artifact_path"),
        ("good", "~/home", 14, "paths", "bad_artifact_path"),
        ("good", "!exclude", 14, "paths", "bad_artifact_path"),
        ("good", "*/glob", 14, "paths", "bad_artifact_path"),
        ("good", "target/~/out", 14, "paths", "bad_artifact_path"),
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
    let reserved = "PATH VELNOR_X NEXTEST_X MISE_X RUSTUP_X CARGO_X GITHUB_X ACTIONS_X RUNNER_X";
    for key in reserved.split_whitespace() {
        let mut stack = RustStackConfig::default_config();
        let mut suite = RustSuiteConfig::new("valid-suite", "default");
        suite.env = BTreeMap::from([(key.to_owned(), "1".to_owned())]);
        stack.suites = vec![suite];
        expect_config_err(
            stack.validate(FILE),
            &format!("stacks.rust.suites[0].env.{key}"),
            &format!("reserved_suite_env_key:{key}"),
        );
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
    let str_input = WorkflowDispatchInputConfig {
        kind: WorkflowDispatchInputType::String,
        description: None,
        required: false,
        default: None,
        options: Vec::new(),
    };
    cfg.workflow.dispatch.inputs = BTreeMap::from([("suite".to_owned(), str_input.clone())]);
    expect_config_err(
        cfg.validate(FILE),
        "workflow.dispatch.inputs.suite",
        "reserved_dispatch_input_name:suite",
    );
    cfg.workflow.dispatch.inputs = BTreeMap::from([("orphan".to_owned(), str_input)]);
    expect_config_err(
        cfg.validate(FILE),
        "workflow.dispatch.inputs.orphan",
        "unused_dispatch_input:orphan",
    );
    cfg.workflow.dispatch.inputs = BTreeMap::from([(
        "group".to_owned(),
        WorkflowDispatchInputConfig {
            kind: WorkflowDispatchInputType::Choice,
            description: None,
            required: false,
            default: Some("fast".to_owned()),
            options: vec!["fast".to_owned()],
        },
    )]);
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
