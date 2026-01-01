//! Plan boundary: built runner plans audit clean, reject foreign
//! volumes, and delete only on exact id match.

use velnor_runner_docker_spec::{
    DeleteDecision, audit_plan, delete_decision, plan_contains, runner_plan,
};
use velnor_runner_journal::HostError;

#[test]
fn runner_plan_rejects_non_private_volumes() -> Result<(), HostError> {
    assert_eq!(runner_plan(""), Err(HostError::ForbiddenMount));
    assert_eq!(runner_plan("a/b"), Err(HostError::ForbiddenMount));
    let plan = runner_plan("worker_a")?;
    assert!(!plan.privileged);
    assert_eq!(plan.platform, "linux/amd64");
    Ok(())
}

#[test]
fn built_plan_audits_clean_and_leak_check_holds() -> Result<(), HostError> {
    let plan = runner_plan("worker_a")?;
    audit_plan(&plan)?;
    assert!(!plan_contains(&plan, "canary-token"));
    assert!(!plan_contains(&plan, ""));
    Ok(())
}

#[test]
fn delete_decision_needs_an_exact_observed_id() {
    assert_eq!(delete_decision("abc", Some("abc")), DeleteDecision::Delete);
    assert_eq!(
        delete_decision("abc", Some("abd")),
        DeleteDecision::KeepForeign
    );
    assert_eq!(delete_decision("abc", None), DeleteDecision::NotDeleted);
    assert_eq!(delete_decision("", Some("")), DeleteDecision::KeepForeign);
}
