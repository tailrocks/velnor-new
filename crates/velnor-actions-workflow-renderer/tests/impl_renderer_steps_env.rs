//! Action-step env validation cases.
use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::action_step_with_env;

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
        velnor_actions_contract::StepKind::Action { env, .. }
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

#[test]
fn cache_mode_requires_default_branch_push_predicate() {
    use velnor_actions_contract::workflow::ir::CACHE_MODE_PUSH_WRITE_EXPR;
    let build = |mode: &str| {
        action_step_with_env(
            "Cache mode",
            &pin("jdx/mr-boxington-action"),
            BTreeMap::new(),
            BTreeMap::from([("ACTIONS_CACHE_MODE".to_owned(), mode.to_owned())]),
        )
    };
    assert!(build(CACHE_MODE_PUSH_WRITE_EXPR).is_ok());
    for weakened in [
        "${{ github.event_name == 'push' && 'write' || 'read' }}",
        "${{ github.event_name == 'pull_request' && 'write' || 'read' }}",
        "${{ github.event_name == 'merge_group' && 'write' || 'read' }}",
        "${{ github.ref == 'refs/heads/main' && 'write' || 'read' }}",
    ] {
        assert!(build(weakened).is_err(), "unsafe mode accepted: {weakened}");
    }
}

#[test]
fn cache_save_input_rejects_event_only_and_untrusted_writers() {
    use velnor_actions_contract::workflow::ir::CACHE_DEFAULT_BRANCH_WRITE_EXPR;
    use velnor_actions_workflow_renderer::action_step;
    let build = |condition: &str| {
        action_step(
            "Build cache",
            &pin("Swatinem/rust-cache"),
            BTreeMap::from([("save-if".to_owned(), condition.to_owned())]),
        )
    };
    assert!(build(CACHE_DEFAULT_BRANCH_WRITE_EXPR).is_ok());
    for weakened in [
        "${{ github.event_name == 'push' }}",
        "${{ github.event_name == 'pull_request' }}",
        "${{ github.event_name == 'merge_group' }}",
        "${{ failure() }}",
        "${{ cancelled() }}",
    ] {
        assert!(
            build(weakened).is_err(),
            "unsafe writer accepted: {weakened}"
        );
    }
    assert!(
        action_step(
            "Build cache",
            &pin("Swatinem/rust-cache"),
            BTreeMap::from([("key".to_owned(), "${{ env.VELNOR_CACHE_IMAGE }}".to_owned())]),
        )
        .is_ok()
    );
}
