//! Action-step env validation cases.
use std::collections::BTreeMap;
use velnor_actions_workflow_steps::action_step_with_env;

fn pin(name: &str) -> String {
    format!("{name}@{:040x}", 0)
}

#[test]
fn action_step_with_env_validates_env_keys_and_values() {
    let env = BTreeMap::from([("MODE".to_owned(), "read".to_owned())]);
    let step = action_step_with_env("Env action", &pin("actions/checkout"), BTreeMap::new(), env)
        .expect("valid env");
    assert!(matches!(
        &step.kind,
        velnor_actions_contract_workflow::StepKind::Action { env, .. }
            if env.get("MODE").is_some_and(|v| v == "read")
    ));
    for bad in [
        BTreeMap::from([("lower".to_owned(), "read".to_owned())]),
        BTreeMap::from([("MODE".to_owned(), "a\nb".to_owned())]),
        BTreeMap::from([("MODE".to_owned(), "velnor-actions __x".to_owned())]),
    ] {
        assert!(
            action_step_with_env("Env action", &pin("actions/checkout"), BTreeMap::new(), bad)
                .is_err(),
            "malformed action env must fail"
        );
    }
}
