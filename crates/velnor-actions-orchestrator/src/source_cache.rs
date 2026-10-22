//! Cargo sources family steps: Plan-only writer, crate-job readers.
//!
//! The Plan job computes the runtime key (`Compute cache keys`), restores,
//! fetches, and saves the shared sources snapshot under a producer gate;
//! crate jobs restore read-only through the Plan job outputs. Every
//! restore and save takes its `path` input from the typed family spec, so
//! the lists are byte-identical by construction.

use velnor_actions_actionlint::actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION};
use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, SOURCES_KEY_PLAN_EXPR, SOURCES_KEY_STEP_EXPR, SOURCES_RESTORE_KEYS_PLAN_EXPR,
    SOURCES_RESTORE_KEYS_STEP_EXPR, SOURCES_SAVE_CONDITION,
};
use velnor_actions_contract::{Step, StepId, StepRole};

use crate::OrchestratorError;

/// Owned Cargo home expression shared by writers and readers.
pub(crate) const SHARED_CARGO_HOME: &str =
    velnor_actions_contract::workflow::cache_family::CARGO_HOME_EXPR;

/// Plan-job restore and producer-gated save over the runtime key.
/// # Errors
///
/// Returns contract, workflow-lint, or render errors for rejected pins,
/// paths, or step shapes.
pub(crate) fn plan_sources_steps() -> Result<(Step, Step), OrchestratorError> {
    let mut restore = sources_step(true, SOURCES_KEY_STEP_EXPR, SOURCES_RESTORE_KEYS_STEP_EXPR)?;
    restore.id = Some(StepId::SourcesRestore);
    let mut save = sources_step(false, SOURCES_KEY_STEP_EXPR, "")?;
    save.condition = Some(SOURCES_SAVE_CONDITION.to_owned());
    Ok((restore, save))
}

/// The one sources restore/save constructor (paths from the family spec).
fn sources_step(restore: bool, key: &str, restore_keys: &str) -> Result<Step, OrchestratorError> {
    use velnor_actions_actionlint::PinnedActionRef;
    use velnor_actions_mise::cache_sources::validate_sources_subset;
    let paths = CacheFamily::Sources.spec().resolved_paths(None);
    validate_sources_subset(&paths, SHARED_CARGO_HOME).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let op = if restore { "restore" } else { "save" };
    let uses = PinnedActionRef::new(
        "actions/cache",
        Some(op),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )
    .map_err(OrchestratorError::from)?
    .uses_value();
    let restore_keys: Vec<String> = if restore {
        vec![restore_keys.to_owned()]
    } else {
        Vec::new()
    };
    let mut step = velnor_actions_workflow_renderer::steps::cache_action_step(
        restore,
        &uses,
        CacheFamily::Sources.spec().layer,
        key,
        &restore_keys,
        &paths,
    )
    .map_err(OrchestratorError::from)?;
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

/// Read-only crate-job restore through the Plan job outputs.
///
/// Lockless emits nothing, and tofu roles restore providers through
/// the separate provider-cache step (never here). The separate V2 tools
/// layer owns Cargo binaries and tool receipts. Compiler driver does not
/// change which shared-source payload readers restore.
pub(crate) fn restore_step_for_crate(
    fetch_roots: &[String],
    use_rust: bool,
) -> Result<Option<Step>, OrchestratorError> {
    if !use_rust || fetch_roots.is_empty() {
        return Ok(None);
    }
    sources_step(true, SOURCES_KEY_PLAN_EXPR, SOURCES_RESTORE_KEYS_PLAN_EXPR).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::StepKind;

    fn with_of(step: &Step) -> &std::collections::BTreeMap<String, String> {
        let StepKind::Action { with, .. } = &step.kind else {
            panic!("cache steps are actions");
        };
        with
    }

    #[test]
    fn plan_and_crate_steps_share_one_spec_path_list() {
        let (restore, save) = plan_sources_steps().expect("plan steps");
        let reader = restore_step_for_crate(&[String::new()], true)
            .expect("reader")
            .expect("lockful");
        let expected = CacheFamily::Sources.spec().path_input(None);
        for step in [&restore, &save, &reader] {
            assert_eq!(with_of(step).get("path"), Some(&expected), "{}", step.name);
            assert!(!with_of(step)["key"].contains("hashFiles"), "{}", step.name);
        }
        assert_eq!(with_of(&restore)["key"], SOURCES_KEY_STEP_EXPR);
        assert_eq!(with_of(&reader)["key"], SOURCES_KEY_PLAN_EXPR);
        assert_eq!(
            with_of(&reader)["restore-keys"],
            SOURCES_RESTORE_KEYS_PLAN_EXPR
        );
        assert_eq!(save.condition.as_deref(), Some(SOURCES_SAVE_CONDITION));
        assert_eq!(restore.id, Some(StepId::SourcesRestore));
        assert!(reader.id.is_none() && reader.condition.is_none());
        assert!(restore_step_for_crate(&[], true).expect("none").is_none());
        assert!(
            restore_step_for_crate(&[String::new()], false)
                .expect("none")
                .is_none()
        );
    }
}
