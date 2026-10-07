//! Release lock, event, and dispatch security contracts.
//!
//! Proves the stable publisher lock (registry/repository/workspace key,
//! no run-unique tokens, never cancel), release-event eligibility
//! (push/schedule/dispatch only; PRs, forks, and `workflow_run`
//! rejected), approved-plan dispatch binding, and the publish gate.
use std::collections::BTreeMap;

use velnor_actions_workflow_release::release_spec::lock::{
    reject_prohibited_release_event, stable_lock_group,
};
use velnor_actions_workflow_release::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseConcurrency, ReleaseTriggers, check_lock_anchor,
    publish_gate_condition,
};
use velnor_actions_workflow_steps::RenderError;

const REPOSITORY: &str = "acme/widgets";
const PLAN_ID: &str = "plan-1";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: PLAN_ID.to_owned(),
        repository: REPOSITORY.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates-io".to_owned(),
        packages: BTreeMap::from([("widgets".to_owned(), "1.2.3".to_owned())]),
        version: None,
    }
}

fn dispatch(name: &str, default: &str) -> DispatchInput {
    DispatchInput {
        name: name.to_owned(),
        description: format!("Approved {name} reference"),
        required: true,
        default: Some(default.to_owned()),
    }
}

fn triggers() -> ReleaseTriggers {
    ReleaseTriggers {
        push_branches: vec!["main".to_owned()],
        schedule: None,
        dispatch_inputs: vec![dispatch("plan", PLAN_ID), dispatch("source_sha", SHA)],
    }
}

fn lock() -> Result<ReleaseConcurrency, RenderError> {
    stable_lock_group("crates-io", REPOSITORY, "root")
}

fn problem_of(result: Result<(), RenderError>) -> Result<String, RenderError> {
    match result {
        Err(RenderError::InvalidWorkflow(problem)) => Ok(problem),
        Err(other) => Err(other),
        Ok(()) => Err(RenderError::InvalidWorkflow(
            "unexpectedly_valid".to_owned(),
        )),
    }
}

#[test]
fn prohibited_events_rejected_by_name() -> Result<(), RenderError> {
    for event in ["push", "schedule", "workflow_dispatch"] {
        assert!(reject_prohibited_release_event(event).is_ok(), "{event}");
    }
    for event in [
        "pull_request",
        "pull_request_target",
        "pull_request_review",
        "workflow_run",
        "merge_group",
        "fork",
        "push ",
        "",
    ] {
        let problem = problem_of(reject_prohibited_release_event(event))?;
        assert!(
            problem.contains("prohibited_release_event"),
            "{event}: {problem}"
        );
    }
    Ok(())
}

#[test]
fn stable_lock_derives_repo_anchored_key() -> Result<(), RenderError> {
    let lock = lock()?;
    assert_eq!(lock.group, "release-crates-io-acme/widgets-root");
    assert!(!lock.cancel_in_progress);
    assert!(lock.validate().is_ok());
    assert!(check_lock_anchor(&lock.group, REPOSITORY).is_ok());
    assert!(stable_lock_group("Crates", REPOSITORY, "root").is_err());
    assert!(stable_lock_group("crates-io", "not-a-repo", "root").is_err());
    assert!(stable_lock_group("crates-io", REPOSITORY, "").is_err());
    assert!(stable_lock_group("crates-io", REPOSITORY, "Root").is_err());
    Ok(())
}

#[test]
fn lock_rejects_unstable_keys_and_cancel() -> Result<(), RenderError> {
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
        let lock = ReleaseConcurrency {
            group: format!("release-x-{token}"),
            cancel_in_progress: false,
        };
        let problem = problem_of(lock.validate())?;
        assert!(
            problem.contains("forbidden_lock_token"),
            "{token}: {problem}"
        );
    }
    let lock = ReleaseConcurrency {
        group: lock()?.group.clone(),
        cancel_in_progress: true,
    };
    assert!(problem_of(lock.validate())?.contains("publisher_cancel"));
    assert!(
        problem_of(check_lock_anchor("release-x-unrelated", REPOSITORY))?
            .contains("unstable_lock_anchor")
    );
    let expression = ReleaseConcurrency {
        group: "release-x-${{ github.repository }}-y".to_owned(),
        cancel_in_progress: false,
    };
    assert!(check_lock_anchor(&expression.group, REPOSITORY).is_ok());
    assert!(problem_of(expression.validate())?.contains("bad_lock_group"));
    Ok(())
}

#[test]
fn dispatch_binding_requires_approved_plan() -> Result<(), RenderError> {
    assert_eq!(DispatchInput::INPUT_TYPE, "string");
    let mut case = triggers();
    case.dispatch_inputs.clear();
    let problem = problem_of(case.validate(&bootstrap()))?;
    assert!(problem.contains("dispatch_plan_mismatch:plan"), "{problem}");
    case = triggers();
    case.dispatch_inputs[0].required = false;
    let problem = problem_of(case.validate(&bootstrap()))?;
    assert!(problem.contains("dispatch_plan_mismatch:plan"), "{problem}");
    case = triggers();
    case.dispatch_inputs[0].default = Some("other".to_owned());
    let problem = problem_of(case.validate(&bootstrap()))?;
    assert!(problem.contains("dispatch_plan_mismatch:plan"), "{problem}");
    case = triggers();
    case.dispatch_inputs.push(dispatch("plan", PLAN_ID));
    let problem = problem_of(case.validate(&bootstrap()))?;
    assert!(problem.contains("duplicate_dispatch_input"), "{problem}");
    case = triggers();
    case.dispatch_inputs.push(DispatchInput {
        name: "Bad Name".to_owned(),
        description: "bad".to_owned(),
        required: false,
        default: None,
    });
    let problem = problem_of(case.validate(&bootstrap()))?;
    assert!(problem.contains("bad_dispatch_name"), "{problem}");
    Ok(())
}

#[test]
fn registry_accepts_hyphenated_names() -> Result<(), RenderError> {
    assert!(bootstrap().validate().is_ok());
    let mut plan = bootstrap();
    plan.registry = "Crates!".to_owned();
    let problem = problem_of(plan.validate())?;
    assert!(problem.contains("bad_registry"), "{problem}");
    plan = bootstrap();
    plan.packages = BTreeMap::new();
    assert!(problem_of(plan.validate())?.contains("no_release_packages"));
    Ok(())
}

#[test]
fn publish_gate_binds_repo_plan_source() {
    let gate = publish_gate_condition(REPOSITORY, &bootstrap());
    assert!(gate.contains(REPOSITORY), "{gate}");
    assert!(gate.contains(PLAN_ID), "{gate}");
    assert!(gate.contains(SHA), "{gate}");
    let fork_gate = publish_gate_condition("evil/fork", &bootstrap());
    assert_ne!(gate, fork_gate, "fork repo must change the gate");
}
