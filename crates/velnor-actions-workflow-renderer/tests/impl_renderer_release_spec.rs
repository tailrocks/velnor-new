//! Release scalar, trigger, concurrency, and bootstrap-plan cases.
use std::collections::BTreeMap;
use velnor_actions_contract::ScheduleTrigger;
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseConcurrency, ReleaseTriggers, check_lock_anchor,
    publish_gate_condition, validate_environment, validate_package_name, validate_package_version,
    validate_plan_id, validate_repository, validate_source_sha,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";
const REPO: &str = "acme/widgets";

/// Extract the `InvalidWorkflow` payload; `None` unless the exact rejection fired.
fn invalid(result: Result<(), RenderError>) -> Option<String> {
    match result {
        Err(RenderError::InvalidWorkflow(text)) => Some(text),
        Err(_) | Ok(()) => None,
    }
}

fn dispatch(name: &str, required: bool, default: Option<&str>) -> DispatchInput {
    DispatchInput {
        name: name.to_owned(),
        description: format!("approved {name}"),
        required,
        default: default.map(str::to_owned),
    }
}

fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: "plan-2026-09-30.1".to_owned(),
        repository: REPO.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates_io".to_owned(),
        packages: BTreeMap::from([("widgets".to_owned(), "1.2.3".to_owned())]),
        version: None,
    }
}

fn triggers() -> ReleaseTriggers {
    ReleaseTriggers {
        push_branches: vec!["main".to_owned()],
        schedule: None,
        dispatch_inputs: vec![
            dispatch("plan", true, Some("plan-2026-09-30.1")),
            dispatch("source_sha", true, Some(SHA)),
        ],
    }
}

#[test]
fn environment_names_pin_without_expressions() {
    for name in ["crates-io", "demo/publish", "env.v2_prod-1"] {
        assert!(validate_environment(name).is_ok(), "for {name}");
    }
    for name in [
        "",
        "has space",
        "bad!",
        "main${{github.sha}}",
        "with\nnewline",
    ] {
        assert_eq!(
            invalid(validate_environment(name)).expect("reject"),
            format!("bad_environment:{name}"),
            "for {name:?}"
        );
    }
}

#[test]
fn repository_identity_requires_exact_owner_repo() {
    for repo in ["acme/widgets", "a.b_c-d/e.f_g-h", "0/1"] {
        assert!(validate_repository(repo).is_ok(), "for {repo}");
    }
    for repo in [
        "",
        "just-owner",
        "a/b/c",
        "/b",
        "a/",
        "a/b c",
        "a/b${{x}}",
        "acme/widgets\n",
    ] {
        assert_eq!(
            invalid(validate_repository(repo)).expect("reject"),
            format!("bad_repository:{repo}"),
            "for {repo:?}"
        );
    }
}

#[test]
fn source_sha_requires_40_lowercase_hex() {
    assert!(validate_source_sha(SHA).is_ok());
    assert!(validate_source_sha(OTHER_SHA).is_ok());
    assert_ne!(SHA, OTHER_SHA);
    let bad = [
        String::new(),
        "abc".to_owned(),
        "Z".repeat(40),
        SHA.to_uppercase(),
        "a".repeat(39),
    ];
    for value in &bad {
        assert_eq!(
            invalid(validate_source_sha(value)).expect("reject"),
            format!("bad_source_sha:{value}"),
            "for {value:?}"
        );
    }
}

#[test]
fn plan_and_package_scalars_reject_shell_shapes() {
    assert!(validate_plan_id("plan-2026-09-30.1").is_ok());
    for id in ["", "has space", "a/b", "x${{y}}", "semi;colon"] {
        assert!(
            invalid(validate_plan_id(id))
                .expect("reject")
                .starts_with("bad_plan_id:")
        );
    }
    assert!(validate_package_name("widgets-2_x").is_ok());
    for name in ["", "-bad", "has space", "a/b", "bad!"] {
        assert!(
            invalid(validate_package_name(name))
                .expect("reject")
                .starts_with("bad_package_name:")
        );
    }
}

#[test]
fn package_version_requires_numeric_triple() {
    for version in ["1.2.3", "0.0.0", "1.2.3-alpha.1", "1.2.3+build.7"] {
        assert!(validate_package_version(version).is_ok(), "for {version}");
    }
    for version in [
        "",
        "1.0",
        "v1.2.3",
        "1.2.3-",
        "1.2.3++",
        "a.b.c",
        "1.2.3 beta",
    ] {
        assert!(
            invalid(validate_package_version(version))
                .expect("reject")
                .starts_with("bad_package_version:")
        );
    }
}

#[test]
fn dispatch_inputs_bind_the_approved_plan_and_source() {
    let plan = bootstrap();
    assert!(triggers().validate(&plan).is_ok());
    let mut rebound = triggers();
    rebound.dispatch_inputs[0].default = Some("other-plan".to_owned());
    assert_eq!(
        invalid(rebound.validate(&plan)).expect("reject"),
        "dispatch_plan_mismatch:plan"
    );
    let mut rebound_sha = triggers();
    rebound_sha.dispatch_inputs[1].default = Some(OTHER_SHA.to_owned());
    assert_eq!(
        invalid(rebound_sha.validate(&plan)).expect("reject"),
        "dispatch_plan_mismatch:source_sha"
    );
    let mut optional = triggers();
    optional.dispatch_inputs[0].required = false;
    assert_eq!(
        invalid(optional.validate(&plan)).expect("reject"),
        "dispatch_plan_mismatch:plan"
    );
    let mut duplicate = triggers();
    duplicate
        .dispatch_inputs
        .push(dispatch("plan", false, None));
    assert!(
        invalid(duplicate.validate(&plan))
            .expect("reject")
            .starts_with("duplicate_dispatch_input:")
    );
    let mut missing = triggers();
    missing.dispatch_inputs.pop();
    assert_eq!(
        invalid(missing.validate(&plan)).expect("reject"),
        "dispatch_plan_mismatch:source_sha"
    );
}

#[test]
fn dispatch_names_reject_uppercase_and_shell_text() {
    for name in ["", "Plan", "has space", "bad!", "a/b", "UPPER"] {
        let input = dispatch(name, true, None);
        assert!(
            invalid(input.validate())
                .expect("reject")
                .starts_with("bad_dispatch_name:")
        );
    }
    let newline = DispatchInput {
        description: "line\nbreak".to_owned(),
        ..dispatch("plan", true, Some("plan-2026-09-30.1"))
    };
    assert!(
        invalid(newline.validate())
            .expect("reject")
            .starts_with("bad_dispatch_description:")
    );
}

#[test]
fn triggers_pin_exact_branches_without_pr_or_fork_events() {
    let plan = bootstrap();
    let mut many = triggers();
    many.push_branches = vec!["main".to_owned(), "release-2.x".to_owned()];
    assert!(many.validate(&plan).is_ok());
    let mut empty = triggers();
    empty.push_branches.clear();
    assert_eq!(
        invalid(empty.validate(&plan)).expect("reject"),
        "no_push_branch"
    );
    for branch in [
        "main*",
        "release?",
        "[abc]",
        "has space",
        "a${{b}}",
        "!main",
        "main;evil",
        "main\non: [push]",
    ] {
        let mut bad = triggers();
        bad.push_branches = vec![branch.to_owned()];
        assert_eq!(
            invalid(bad.validate(&plan)).expect("reject"),
            format!("bad_push_branch:{branch}"),
            "for {branch:?}"
        );
    }
    let mut scheduled = triggers();
    scheduled.schedule = Some(ScheduleTrigger {
        cron: vec!["0 6 * * 1".to_owned()],
    });
    assert!(scheduled.validate(&plan).is_ok());
    let mut bad_cron = triggers();
    bad_cron.schedule = Some(ScheduleTrigger {
        cron: vec!["nope".to_owned()],
    });
    assert!(bad_cron.validate(&plan).is_err());
}

#[test]
fn concurrency_queues_on_a_stable_lock_and_never_cancels() {
    let stable = ReleaseConcurrency {
        group: "release-acme/widgets".to_owned(),
        cancel_in_progress: false,
    };
    assert!(stable.validate().is_ok());
    let cancelling = ReleaseConcurrency {
        cancel_in_progress: true,
        ..stable.clone()
    };
    assert_eq!(
        invalid(cancelling.validate()).expect("reject"),
        "publisher_cancel"
    );
    for token in [
        "run_id",
        "run_attempt",
        "run_number",
        "github.sha",
        "github.ref",
        "github.event",
        "inputs.",
        "matrix.",
        "version",
        "strategy",
    ] {
        let unstable = ReleaseConcurrency {
            group: format!("release-{token}-widgets"),
            cancel_in_progress: false,
        };
        assert_eq!(
            invalid(unstable.validate()).expect("reject"),
            format!("forbidden_lock_token:{token}"),
            "for {token}"
        );
    }
    assert_eq!(
        invalid(stable_no_group().validate()).expect("reject"),
        "bad_lock_group:"
    );
}

fn stable_no_group() -> ReleaseConcurrency {
    ReleaseConcurrency {
        group: String::new(),
        cancel_in_progress: false,
    }
}

#[test]
fn lock_anchor_requires_the_repository_identity() {
    assert!(check_lock_anchor("release-acme/widgets", REPO).is_ok());
    assert!(check_lock_anchor("release-github.repository-widgets", REPO).is_ok());
    assert_eq!(
        invalid(check_lock_anchor("release-widgets-1", REPO)).expect("reject"),
        "unstable_lock_anchor"
    );
}

#[test]
fn bootstrap_plan_validates_every_identity() {
    assert!(bootstrap().validate().is_ok());
    let mut bad_repo = bootstrap();
    bad_repo.repository = "no-slash".to_owned();
    assert!(
        invalid(bad_repo.validate())
            .expect("reject")
            .starts_with("bad_repository:")
    );
    let mut bad_sha = bootstrap();
    bad_sha.source_sha = "short".to_owned();
    assert!(
        invalid(bad_sha.validate())
            .expect("reject")
            .starts_with("bad_source_sha:")
    );
    let mut bad_registry = bootstrap();
    bad_registry.registry = "Crates!".to_owned();
    assert!(
        invalid(bad_registry.validate())
            .expect("reject")
            .starts_with("bad_registry:")
    );
    let mut empty = bootstrap();
    empty.packages.clear();
    assert_eq!(
        invalid(empty.validate()).expect("reject"),
        "no_release_packages"
    );
    let mut bad_member = bootstrap();
    bad_member.packages = BTreeMap::from([("bad name".to_owned(), "1.2.3".to_owned())]);
    assert!(
        invalid(bad_member.validate())
            .expect("reject")
            .starts_with("bad_package_name:")
    );
    let mut bad_version = bootstrap();
    bad_version.packages = BTreeMap::from([("widgets".to_owned(), "1.0".to_owned())]);
    assert!(
        invalid(bad_version.validate())
            .expect("reject")
            .starts_with("bad_package_version:")
    );
}

#[test]
fn publish_gate_binds_repo_plan_and_source_exactly() {
    let plan = bootstrap();
    let gate = publish_gate_condition(REPO, &plan);
    assert_eq!(
        gate,
        format!(
            "github.repository == '{REPO}' && github.event.inputs.plan == '{}' && github.event.inputs.source_sha == '{SHA}'",
            plan.plan_id,
        ),
        "gate snapshot"
    );
    assert!(!gate.contains("github.sha"), "never the workflow SHA");
    assert!(!gate.contains("github.ref"), "never a mutable ref");
    let forked = publish_gate_condition("mallory/widgets", &plan);
    assert_ne!(gate, forked, "forks fail the gate");
}
