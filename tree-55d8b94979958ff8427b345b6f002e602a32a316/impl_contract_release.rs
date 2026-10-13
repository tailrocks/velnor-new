//! Typed release config and release IR cases (synthetic demo data only).
use std::collections::BTreeMap;
use velnor_actions_contract::config::{BootstrapRelease, ReleaseAuthentication, RustReleaseConfig};
use velnor_actions_contract::workflow::ir::{
    Concurrency, DispatchInput, Job, PermissionLevel, Permissions, Step, StepKind, Trigger,
    WorkflowDispatch, WorkflowIr,
};
use velnor_actions_contract::{ContractError, ScheduleTrigger, canonical_json_str};

const FILE: &str = ".velnor/config.toml";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn valid_release() -> RustReleaseConfig {
    RustReleaseConfig {
        enabled: true,
        manifest_path: "Cargo.toml".to_owned(),
        packages: vec!["demo-crate".to_owned()],
        environment: "demo-publish".to_owned(),
        authentication: ReleaseAuthentication::TrustedPublishing,
        release_pr: true,
        tag_name: "{{ package }}-v{{ version }}".to_owned(),
        bootstrap: None,
        version_groups: BTreeMap::new(),
    }
}

fn valid_bootstrap() -> BootstrapRelease {
    BootstrapRelease {
        package: "demo-crate".to_owned(),
        version: "1.2.3".to_owned(),
        source_sha: SHA.to_owned(),
    }
}

fn bootstrap_release(mutate: impl FnOnce(&mut BootstrapRelease)) -> RustReleaseConfig {
    let mut config = valid_release();
    config.authentication = ReleaseAuthentication::BootstrapToken;
    let mut bootstrap = valid_bootstrap();
    mutate(&mut bootstrap);
    config.bootstrap = Some(bootstrap);
    config
}

fn input(name: &str, required: bool, default: Option<&str>) -> DispatchInput {
    DispatchInput {
        name: name.to_owned(),
        required,
        default: default.map(str::to_owned),
    }
}

fn config_problem(config: &RustReleaseConfig) -> Option<String> {
    match config.validate(FILE) {
        Err(ContractError::Config {
            key_path, problem, ..
        }) => Some(format!("{key_path} {problem}")),
        _ => None,
    }
}

fn identity_problem(workflow: &WorkflowIr) -> Option<String> {
    match workflow.validate() {
        Err(ContractError::InvalidIdentity { field, problem }) => {
            Some(format!("{field} {problem}"))
        }
        _ => None,
    }
}

fn decode_problem(document: &str) -> Option<String> {
    let Err(decode) = serde_json::from_str::<RustReleaseConfig>(document) else {
        return None;
    };
    match ContractError::map_decode_error(FILE, &decode.to_string()) {
        ContractError::Config { problem, .. } => Some(problem),
        _ => None,
    }
}

#[test]
fn release_disabled_by_default_and_enabled_needs_allowlist() {
    let default = RustReleaseConfig::default();
    assert!(!default.enabled);
    assert!(default.packages.is_empty());
    assert_eq!(default.validate(FILE), Ok(()));
    assert_eq!(
        default.authentication,
        ReleaseAuthentication::TrustedPublishing
    );
    assert_eq!(valid_release().validate(FILE), Ok(()));
    let mut empty = valid_release();
    empty.packages.clear();
    let got = config_problem(&empty).expect("must reject");
    assert_eq!(got, "stacks.rust.release.packages empty_packages");
}

#[test]
fn release_rejects_unknown_fields_without_shell_yaml_uses() {
    for document in [
        r#"{"enabled":true,"shell":"cargo publish"}"#,
        r#"{"enabled":true,"uses":"some/action@ref"}"#,
        r#"{"enabled":true,"run":["cargo","publish"]}"#,
        r#"{"enabled":true,"bootstrap":{"package":"demo-crate","token":"abc"}}"#,
    ] {
        assert_eq!(
            decode_problem(document).expect("must reject"),
            "unknown_config_field",
            "for {document}"
        );
    }
}

#[test]
fn release_rejects_duplicate_unsorted_and_unsafe_selection() {
    let mut duplicate = valid_release();
    duplicate.packages = vec!["demo-crate".to_owned(), "demo-crate".to_owned()];
    assert!(
        config_problem(&duplicate)
            .expect("must reject")
            .ends_with("duplicate_package")
    );
    let mut unsorted = valid_release();
    unsorted.packages = vec!["demo-crate".to_owned(), "aaa-crate".to_owned()];
    assert!(
        config_problem(&unsorted)
            .expect("must reject")
            .ends_with("must_be_sorted")
    );
    for name in "|../escape|a/b|has space|9bad|-bad|bad!|bad;run|bad$(x)|..".split('|') {
        let mut unsafe_name = valid_release();
        unsafe_name.packages = vec![name.to_owned()];
        let got = config_problem(&unsafe_name).expect("must reject");
        assert_eq!(
            got,
            format!("stacks.rust.release.packages unsafe_package:{name}")
        );
    }
}

#[test]
fn release_rejects_contradictory_authentication_modes() {
    let mut missing = valid_release();
    missing.authentication = ReleaseAuthentication::BootstrapToken;
    let got = config_problem(&missing).expect("must reject");
    assert_eq!(
        got,
        "stacks.rust.release.bootstrap missing_bootstrap_record"
    );
    let mut contradictory = valid_release();
    contradictory.bootstrap = Some(valid_bootstrap());
    let got = config_problem(&contradictory).expect("must reject");
    assert_eq!(
        got,
        "stacks.rust.release.bootstrap contradictory_authentication"
    );
    assert_eq!(bootstrap_release(|_| {}).validate(FILE), Ok(()));
}

#[test]
fn release_rejects_bootstrap_mismatch_fail_closed() {
    let bad_package = bootstrap_release(|bootstrap| bootstrap.package = "../evil".to_owned());
    assert!(
        config_problem(&bad_package)
            .expect("must reject")
            .contains("unsafe_package:")
    );
    for version in ["1.0", "v1.2.3", "1.2.3-beta", "1.2.3+build", "a.b.c", ""] {
        let bad = bootstrap_release(|bootstrap| bootstrap.version = version.to_owned());
        let got = config_problem(&bad).expect("must reject");
        let want = format!("stacks.rust.release.bootstrap.version bad_version:{version}");
        assert_eq!(got, want, "for {version:?}");
    }
    let non_hex = "Z".repeat(40);
    let upper = SHA.to_uppercase();
    for sha in ["abc", non_hex.as_str(), upper.as_str(), ""] {
        let bad = bootstrap_release(|bootstrap| bootstrap.source_sha = sha.to_owned());
        let got = config_problem(&bad).expect("must reject");
        assert_eq!(
            got,
            "stacks.rust.release.bootstrap.source_sha bad_source_sha"
        );
    }
}

#[test]
fn release_validates_manifest_environment_and_tag() {
    for path in "|/abs/Cargo.toml|../up/Cargo.toml|crates/a|a\\Cargo.toml".split('|') {
        let mut bad = valid_release();
        bad.manifest_path = path.to_owned();
        assert!(
            config_problem(&bad)
                .expect("must reject")
                .starts_with("stacks.rust.release.manifest_path ")
        );
    }
    let mut nested = valid_release();
    nested.manifest_path = "crates/demo/Cargo.toml".to_owned();
    assert_eq!(nested.validate(FILE), Ok(()));
    for env in ["", " padded", "bad env!", "../x", "a//b"] {
        let mut bad = valid_release();
        bad.environment = env.to_owned();
        assert!(
            config_problem(&bad)
                .expect("must reject")
                .starts_with("stacks.rust.release.environment ")
        );
    }
    for tag in "v{{ version }}|{{ package }}-v1.0||{{ package }}-$(x)-{{ version }}|{{ package }}-`x`-{{ version }}|{{ package }}-v{{ version }".split('|') {
        let mut bad = valid_release();
        bad.tag_name = tag.to_owned();
        assert!(config_problem(&bad).expect("must reject").starts_with("stacks.rust.release.tag_name "));
    }
}

#[test]
fn release_version_groups_are_non_lockstep_and_allowlist_bound() {
    let mut grouped = valid_release();
    grouped.packages = vec!["aaa-crate".to_owned(), "demo-crate".to_owned()];
    grouped.version_groups = BTreeMap::from([(
        "core".to_owned(),
        vec!["aaa-crate".to_owned(), "demo-crate".to_owned()],
    )]);
    assert_eq!(grouped.validate(FILE), Ok(()));
    let mut unknown = grouped.clone();
    unknown.version_groups = BTreeMap::from([("core".to_owned(), vec!["ghost".to_owned()])]);
    assert!(
        config_problem(&unknown)
            .expect("must reject")
            .ends_with("unknown_package:ghost")
    );
    let mut split = grouped.clone();
    split.version_groups = BTreeMap::from([
        ("one".to_owned(), vec!["demo-crate".to_owned()]),
        ("two".to_owned(), vec!["demo-crate".to_owned()]),
    ]);
    assert!(
        config_problem(&split)
            .expect("must reject")
            .ends_with("member_in_two_groups:demo-crate")
    );
    let mut empty = grouped.clone();
    empty.version_groups = BTreeMap::from([("core".to_owned(), vec![])]);
    assert!(
        config_problem(&empty)
            .expect("must reject")
            .ends_with("empty_group")
    );
    let mut bad_group = grouped;
    let groups = BTreeMap::from([("Bad group".to_owned(), vec!["demo-crate".to_owned()])]);
    bad_group.version_groups = groups;
    assert!(
        config_problem(&bad_group)
            .expect("must reject")
            .ends_with("unsafe_group:Bad group")
    );
}

#[test]
fn release_wired_into_stack_validation_with_key_paths() {
    use velnor_actions_contract::{RustConfiguration, RustStackConfig};
    let stack = RustStackConfig {
        configurations: vec![RustConfiguration {
            name: "default".to_owned(),
            features: vec!["default".to_owned()],
            target: "host".to_owned(),
        }],
        compile_driver: None,
        test_runner: None,
        release: valid_release(),
    };
    assert_eq!(stack.validate(FILE), Ok(()));
    let mut bad = stack.clone();
    bad.release.packages.clear();
    let Err(ContractError::Config { key_path, .. }) = bad.validate(FILE) else {
        panic!("empty enabled allowlist must fail through the stack");
    };
    assert_eq!(key_path, "stacks.rust.release.packages");
    assert_eq!(RustStackConfig::default_config().validate(FILE), Ok(()));
}

#[test]
fn release_config_is_deterministic() {
    let first = canonical_json_str(&valid_release()).expect("canonical");
    let second = canonical_json_str(&valid_release()).expect("canonical");
    assert_eq!(first, second);
    let roundtrip: RustReleaseConfig = serde_json::from_str(&first).expect("roundtrip");
    assert_eq!(roundtrip, valid_release());
    assert_eq!(canonical_json_str(&roundtrip).expect("canonical"), first);
}

fn ci_triggers() -> Trigger {
    Trigger {
        pull_request_types: vec!["opened".to_owned()],
        push_branches: vec!["main".to_owned()],
        merge_group: false,
        workflow_dispatch: None,
        schedule: None,
    }
}

fn ci_job() -> Job {
    Job {
        display_name: "demo check".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        needs: vec![],
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: "run".to_owned(),
            kind: StepKind::Internal {
                operation: "demo".to_owned(),
            },
        }],
    }
}

fn ci_workflow() -> WorkflowIr {
    WorkflowIr {
        name: "demo CI".to_owned(),
        triggers: ci_triggers(),
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "demo".to_owned(),
            cancel_in_progress: "false".to_owned(),
        },
        jobs: BTreeMap::from([("check".to_owned(), ci_job())]),
    }
}

fn check_job(workflow: &mut WorkflowIr) -> Option<&mut Job> {
    workflow.jobs.get_mut("check")
}

fn dispatch_inputs(workflow: &mut WorkflowIr) -> Option<&mut Vec<DispatchInput>> {
    workflow
        .triggers
        .workflow_dispatch
        .as_mut()
        .map(|dispatch| &mut dispatch.inputs)
}

#[test]
fn ir_default_permissions_preserve_read_read_ci() {
    let permissions = Permissions::default();
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.actions, PermissionLevel::Read);
    assert_eq!(permissions.pull_requests, PermissionLevel::None);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    assert!(!permissions.is_write_all());
    assert_eq!(ci_workflow().validate(), Ok(()));
}

#[test]
fn ir_rejects_permission_violations() {
    let mut id_token = ci_workflow();
    id_token.permissions.id_token = PermissionLevel::Write;
    let got = identity_problem(&id_token).expect("must reject");
    assert_eq!(
        got,
        "job.environment id_token_write_needs_environment:check"
    );
    let mut bound = id_token.clone();
    check_job(&mut bound).expect("job").environment = Some("demo-publish".to_owned());
    bound.triggers.pull_request_types.clear();
    assert_eq!(bound.validate(), Ok(()));
    let mut on_pr = ci_workflow();
    let scoped = Permissions {
        contents: PermissionLevel::Write,
        ..Permissions::default()
    };
    check_job(&mut on_pr).expect("job").permissions = Some(scoped);
    let got = identity_problem(&on_pr).expect("must reject");
    assert_eq!(got, "job.permissions contents_write_on_pr:check");
    let write_all = Permissions {
        contents: PermissionLevel::Write,
        pull_requests: PermissionLevel::Write,
        id_token: PermissionLevel::Write,
        actions: PermissionLevel::Write,
    };
    let mut workflow_all = ci_workflow();
    workflow_all.permissions = write_all.clone();
    assert_eq!(
        identity_problem(&workflow_all).expect("must reject"),
        "workflow.permissions write_all"
    );
    let mut job_all = ci_workflow();
    check_job(&mut job_all).expect("job").permissions = Some(write_all);
    assert!(
        identity_problem(&job_all)
            .expect("must reject")
            .ends_with("write_all:check")
    );
}

#[test]
fn ir_validates_dispatch_input_charset_and_order() {
    assert_eq!(DispatchInput::INPUT_TYPE, "string");
    let mut workflow = ci_workflow();
    workflow.triggers.pull_request_types.clear();
    workflow.triggers.workflow_dispatch = Some(WorkflowDispatch {
        inputs: vec![
            input("package", true, None),
            input("source-sha", false, Some(SHA)),
        ],
    });
    assert_eq!(workflow.validate(), Ok(()));
    for name in ["", "Bad", "has space", "bad!", "bad/x", "UPPER"] {
        let mut bad = workflow.clone();
        dispatch_inputs(&mut bad).expect("dispatch")[0].name = name.to_owned();
        let got = identity_problem(&bad).expect("must reject");
        assert_eq!(got, format!("trigger.dispatch.inputs.name bad_name:{name}"));
    }
    let mut duplicate = workflow.clone();
    dispatch_inputs(&mut duplicate).expect("dispatch")[1].name = "package".to_owned();
    assert!(
        identity_problem(&duplicate)
            .expect("must reject")
            .ends_with("duplicate_input")
    );
    let mut unsorted = workflow.clone();
    *dispatch_inputs(&mut unsorted).expect("dispatch") =
        vec![input("zz", true, None), input("aa", true, None)];
    assert!(
        identity_problem(&unsorted)
            .expect("must reject")
            .ends_with("must_be_sorted")
    );
    let mut bad_default = workflow;
    dispatch_inputs(&mut bad_default).expect("dispatch")[1].default =
        Some("has\nnewline".to_owned());
    assert!(
        identity_problem(&bad_default)
            .expect("must reject")
            .ends_with("bad_default:source-sha")
    );
}

#[test]
fn ir_validates_schedule_and_environment_safety() {
    let mut scheduled = ci_workflow();
    scheduled.triggers.schedule = Some(ScheduleTrigger {
        cron: vec!["0 6 * * 1".to_owned()],
    });
    assert_eq!(scheduled.validate(), Ok(()));
    let mut bad_cron = scheduled.clone();
    bad_cron.triggers.schedule.as_mut().expect("schedule").cron = vec!["nope".to_owned()];
    assert!(bad_cron.validate().is_err());
    let mut bad_env = scheduled;
    check_job(&mut bad_env).expect("job").environment = Some("../evil".to_owned());
    assert!(
        identity_problem(&bad_env)
            .expect("must reject")
            .ends_with("bad_environment:check")
    );
}
