//! `[workflow] checkout_lfs` pass tests.

use super::*;

#[test]
fn checkout_lfs_pass_marks_only_checkout_steps() {
    use velnor_actions_contract::{Job, JobTimeout};
    let other = Step {
        parallel: false,
        name: "Other".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Action {
            uses: "example/other@0000000000000000000000000000000000000000".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "a".to_owned(),
        Job {
            check_runner: None,
            display_name: "Demo".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: JobTimeout::CRATE,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            cache_mode: None,
            steps: vec![
                checkout_step().expect("checkout"),
                checkout_step_full().expect("full checkout"),
                other,
            ],
        },
    );
    apply_checkout_lfs(&mut jobs);
    let steps = &jobs["a"].steps;
    for step in &steps[..2] {
        let StepKind::Action { with, .. } = &step.kind else {
            panic!("checkout action");
        };
        assert_eq!(with.get("lfs").map(String::as_str), Some("true"));
        assert!(validate_action_inputs(&checkout_inputs_schema(), with).is_ok());
        assert!(
            velnor_actions_contract::workflow::step_identity::is_configured_checkout(
                step,
                &PinnedActionRef::checkout().uses_value(),
            )
        );
    }
    let StepKind::Action { with, .. } = &steps[2].kind else {
        panic!("other action");
    };
    assert!(with.is_empty(), "non-checkout steps stay untouched");
}
