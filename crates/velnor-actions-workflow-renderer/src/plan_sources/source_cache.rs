// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_actionlint::actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION};
use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, SOURCES_KEY_PLAN_EXPR, SOURCES_KEY_STEP_EXPR, SOURCES_RESTORE_KEYS_PLAN_EXPR,
    SOURCES_RESTORE_KEYS_STEP_EXPR, SOURCES_SAVE_CONDITION,
};
use velnor_actions_contract::{Step, StepId, StepRole};

use super::PlanSourcesError;

pub const SHARED_CARGO_HOME: &str =
    velnor_actions_contract::workflow::cache_family::CARGO_HOME_EXPR;

pub fn plan_sources_steps() -> Result<(Step, Step), PlanSourcesError> {
    let mut restore = sources_step(true, SOURCES_KEY_STEP_EXPR, SOURCES_RESTORE_KEYS_STEP_EXPR)?;
    restore.id = Some(StepId::SourcesRestore);
    let mut save = sources_step(false, SOURCES_KEY_STEP_EXPR, "")?;
    save.condition = Some(SOURCES_SAVE_CONDITION.to_owned());
    Ok((restore, save))
}

pub fn restore_step_for_crate(
    fetch_roots: &[String],
    use_rust: bool,
) -> Result<Option<Step>, PlanSourcesError> {
    if !use_rust || fetch_roots.is_empty() {
        return Ok(None);
    }
    sources_step(true, SOURCES_KEY_PLAN_EXPR, SOURCES_RESTORE_KEYS_PLAN_EXPR).map(Some)
}

fn sources_step(restore: bool, key: &str, restore_keys: &str) -> Result<Step, PlanSourcesError> {
    use velnor_actions_actionlint::PinnedActionRef;
    use velnor_actions_mise::cache_sources::validate_sources_subset;

    let paths = CacheFamily::Sources.spec().resolved_paths(None);
    validate_sources_subset(&paths, SHARED_CARGO_HOME)
        .map_err(|err| PlanSourcesError::CacheSourcesPolicy(err.to_string()))?;
    let op = if restore { "restore" } else { "save" };
    let uses = PinnedActionRef::new(
        "actions/cache",
        Some(op),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )
    .map_err(|err| PlanSourcesError::CacheActionRef(err.to_string()))?
    .uses_value();
    let restore_keys: Vec<String> = if restore {
        vec![restore_keys.to_owned()]
    } else {
        Vec::new()
    };
    let mut step = crate::steps::cache_action_step(
        restore,
        &uses,
        CacheFamily::Sources.spec().layer,
        key,
        &restore_keys,
        &paths,
    )
    .map_err(PlanSourcesError::Render)?;
    step.name = if restore {
        "Restore Cargo sources".to_owned()
    } else {
        "Save Cargo sources".to_owned()
    };
    step.role = Some(if restore {
        StepRole::CargoSourcesRestore
    } else {
        StepRole::CargoSourcesSave
    });
    Ok(step)
}
