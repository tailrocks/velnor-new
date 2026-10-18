use std::collections::BTreeMap;

use super::{StepId, StepRole, TOOLS_CACHE_RESTORE_USES, validate_step_sequence};
use crate::workflow::{Step, StepKind};

fn restore_step(id: Option<StepId>, role: Option<StepRole>, uses: &str) -> Step {
    Step {
        name: "Restore tools cache".to_owned(),
        id,
        role,
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn tools_cache_restore_requires_typed_outer_id() {
    let step = restore_step(
        Some(StepId::ToolsCacheRestore),
        Some(StepRole::ToolsCacheRestore),
        TOOLS_CACHE_RESTORE_USES,
    );

    assert!(validate_step_sequence(&[step], "tools-cache").is_ok());
}

#[test]
fn tools_cache_restore_rejects_missing_id_and_wrong_action() {
    let missing_id = restore_step(
        None,
        Some(StepRole::ToolsCacheRestore),
        TOOLS_CACHE_RESTORE_USES,
    );
    let wrong_action = restore_step(
        Some(StepId::ToolsCacheRestore),
        Some(StepRole::ToolsCacheRestore),
        "./.github/actions/other",
    );

    assert!(validate_step_sequence(&[missing_id], "tools-cache").is_err());
    assert!(validate_step_sequence(&[wrong_action], "tools-cache").is_err());
}
