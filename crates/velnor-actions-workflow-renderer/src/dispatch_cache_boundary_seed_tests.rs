use std::collections::BTreeMap;

use super::super::{DISPATCH_DENY, suppress_unvalidated_cache_access};
use super::{cache_step, job};
use velnor_actions_contract::{Step, StepKind};

#[test]
fn dispatch_denies_the_local_tool_seed_action() {
    let seed = Step {
        name: crate::tool_seed::TOOL_SEED_NAME.to_owned(),
        id: None,
        role: Some(velnor_actions_contract::StepRole::ToolSeed),
        condition: None,
        kind: StepKind::Action {
            uses: crate::tool_seed::TOOL_SEED_USES.to_owned(),
            with: BTreeMap::from([(
                "cache_key".to_owned(),
                crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION.to_owned(),
            )]),
            env: BTreeMap::new(),
        },
    };
    let mut jobs = BTreeMap::from([("plan".to_owned(), job(&[], seed))]);

    suppress_unvalidated_cache_access(&mut jobs);

    assert!(jobs["plan"].steps[0].condition.is_none());
    let StepKind::Action { with, .. } = &jobs["plan"].steps[0].kind else {
        panic!("tool seed remains an action");
    };
    let key = with
        .get("cache_key")
        .expect("cache key is disabled by input");
    assert!(crate::tool_seed::is_guarded_seed_key(key));
    assert!(key.contains(DISPATCH_DENY));
}

#[test]
fn dispatch_denies_native_mbx_object_restore_before_use() {
    let native = cache_step(
        &format!("{}@{}", crate::cache_steps::MBX_ACTION_NAME, "a".repeat(40)),
        "Restore MBX objects",
    );
    let mut jobs = BTreeMap::from([("plan".to_owned(), job(&[], native))]);

    suppress_unvalidated_cache_access(&mut jobs);

    assert_eq!(
        jobs["plan"].steps[0].condition.as_deref(),
        Some(DISPATCH_DENY),
        "native MBX restore is skipped on every workflow dispatch"
    );
}
