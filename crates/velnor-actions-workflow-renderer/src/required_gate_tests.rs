//! The protected `Required` check can never be satisfied by a cancelled run.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    Job, JobTimeout, REQUIRED_DISPLAY_NAME, REQUIRED_GATE_CONDITION, REQUIRED_GATE_FAIL_CONDITION,
    REQUIRED_GATE_JOB_ID, REQUIRED_VERDICT_DISPLAY_NAME,
};

use super::{check_required_gate, insert_required_gate};
use crate::render::{FINAL_CONDITION, FINAL_JOB_ID};

fn verdict_jobs() -> BTreeMap<String, Job> {
    let verdict = Job {
        display_name: REQUIRED_VERDICT_DISPLAY_NAME.to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs: vec!["plan".to_owned()],
        condition: Some(FINAL_CONDITION.to_owned()),
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: Vec::new(),
    };
    BTreeMap::from([(FINAL_JOB_ID.to_owned(), verdict)])
}

#[test]
fn gate_always_runs_and_fails_unless_the_verdict_succeeded() {
    let mut jobs = verdict_jobs();
    insert_required_gate(&mut jobs).expect("gate inserts");
    let gate = jobs.get(REQUIRED_GATE_JOB_ID).expect("gate job");
    assert_eq!(gate.display_name, REQUIRED_DISPLAY_NAME);
    assert_eq!(gate.condition.as_deref(), Some("${{ always() }}"));
    assert_eq!(gate.needs, vec![FINAL_JOB_ID.to_owned()]);
    let [step] = gate.steps.as_slice() else {
        panic!("one step: {:?}", gate.steps);
    };
    assert_eq!(
        step.condition.as_deref(),
        Some("${{ always() && needs.required.result != 'success' }}"),
        "the step never inherits success(), so a cancelled run cannot skip it"
    );
    assert!(check_required_gate(&jobs).is_ok());
}

#[test]
fn verdict_keeps_the_cancellable_condition_and_its_own_name() {
    assert_eq!(FINAL_CONDITION, "${{ !cancelled() }}");
    assert_ne!(REQUIRED_VERDICT_DISPLAY_NAME, REQUIRED_DISPLAY_NAME);
    assert_ne!(REQUIRED_GATE_CONDITION, FINAL_CONDITION);
    assert!(REQUIRED_GATE_FAIL_CONDITION.starts_with("${{ always() &&"));
}

#[test]
fn missing_or_altered_gates_fail_generation() {
    let jobs = verdict_jobs();
    assert!(check_required_gate(&jobs).is_err(), "missing gate");
    let mut altered = jobs.clone();
    insert_required_gate(&mut altered).expect("gate inserts");
    if let Some(gate) = altered.get_mut(REQUIRED_GATE_JOB_ID) {
        gate.condition = Some(FINAL_CONDITION.to_owned());
    }
    assert!(check_required_gate(&altered).is_err(), "cancellable gate");
    let mut passing = jobs.clone();
    insert_required_gate(&mut passing).expect("gate inserts");
    if let Some(step) = passing
        .get_mut(REQUIRED_GATE_JOB_ID)
        .and_then(|gate| gate.steps.first_mut())
    {
        step.kind = velnor_actions_contract::StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        };
    }
    assert!(
        check_required_gate(&passing).is_err(),
        "gate step must fail"
    );
    let mut renamed = jobs;
    if let Some(verdict) = renamed.get_mut(FINAL_JOB_ID) {
        verdict.display_name = REQUIRED_DISPLAY_NAME.to_owned();
    }
    insert_required_gate(&mut renamed).expect("gate inserts");
    assert!(
        check_required_gate(&renamed).is_err(),
        "two Required checks"
    );
}
