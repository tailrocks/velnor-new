//! Dispatch-boundary condition-preservation tests.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use super::suppress_unvalidated_cache_access;
use super::tests::{cache_step, job};

#[test]
fn suppression_preserves_existing_cache_step_condition() {
    let mut step = cache_step("actions/cache/save@sha", "sources");
    step.condition = Some("success() && github.event_name == 'push'".to_owned());
    let mut jobs = BTreeMap::from([("plan".to_owned(), job(&[], step))]);

    suppress_unvalidated_cache_access(&mut jobs);

    assert_eq!(
        jobs["plan"].steps[0].condition.as_deref(),
        Some("success() && github.event_name == 'push'")
    );
}

#[test]
fn dispatch_suppression_preserves_canonical_push_only_save_gate() {
    let save = Step {
        name: "Save Tofu providers".to_owned(),
        id: None,
        role: Some(velnor_actions_contract::StepRole::TofuProvidersSave),
        condition: Some(
            velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION.to_owned(),
        ),
        kind: StepKind::Action {
            uses: "actions/cache/save@sha".to_owned(),
            with: BTreeMap::from([
                (
                    "key".to_owned(),
                    velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_KEY_OUTPUT_EXPR
                        .to_owned(),
                ),
                (
                    "path".to_owned(),
                    velnor_actions_contract::workflow::step_identity::TOFU_PROVIDERS_PATH_OUTPUT_EXPR
                        .to_owned(),
                ),
            ]),
            env: BTreeMap::new(),
        },
    };
    let mut jobs = BTreeMap::from([("plan".to_owned(), job(&[], save))]);

    suppress_unvalidated_cache_access(&mut jobs);

    assert_eq!(
        jobs["plan"].steps[0].condition.as_deref(),
        Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION),
        "the exact push-only gate already denies workflow_dispatch"
    );
}

#[test]
fn dispatch_suppression_preserves_qualified_tools_save_gate() {
    let mut save = cache_step("actions/cache/save@sha", "Save Mise tools");
    save.role = Some(velnor_actions_contract::StepRole::ToolsCacheSave);
    save.condition = Some(crate::cache_p08::tools_cache_save_condition());
    let mut jobs = BTreeMap::from([("plan".to_owned(), job(&[], save))]);

    suppress_unvalidated_cache_access(&mut jobs);

    assert_eq!(
        jobs["plan"].steps[0].condition.as_deref(),
        Some(crate::cache_p08::tools_cache_save_condition().as_str()),
        "the validated tool-cache save policy already restricts writers to protected default-branch pushes"
    );
}

#[test]
fn dispatch_deny_remains_outermost_for_existing_disjunctions() {
    let mut step = cache_step("actions/cache/save@sha", "sources");
    step.condition = Some("(github.event_name != 'workflow_dispatch' || always())".to_owned());
    let mut jobs = BTreeMap::from([("plan".to_owned(), job(&[], step))]);

    suppress_unvalidated_cache_access(&mut jobs);

    assert_eq!(
        jobs["plan"].steps[0].condition.as_deref(),
        Some(
            "((github.event_name != 'workflow_dispatch' || always())) && github.event_name != 'workflow_dispatch'"
        )
    );
}
