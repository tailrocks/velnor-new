//! Read-only exact-key observer for validated qualification child runs.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use super::pr_cache::QualificationObserverBinding;
use super::{
    IMPORT_SCRIPT, MBX_BUNDLE_IMPORT_NAME, MBX_BUNDLE_KEY_NAME, identity, pr_cache, qualification,
    restore_step,
};
use crate::RenderError;

/// Build the exact-only reader lifecycle for a validated qualification child.
///
/// # Errors
/// Invalid scope or key identity fails closed before emitting steps.
pub(crate) fn qualification_observer_steps(
    scope: &str,
    version: &str,
    rust_env: &BTreeMap<String, String>,
    binding: &QualificationObserverBinding<'_>,
) -> Result<Vec<Step>, RenderError> {
    let generation = velnor_actions_contract::cachekey::mbx_cache_generation(version);
    if !identity::valid_qualification_scope(scope)
        || rust_env
            .get("RUSTUP_TOOLCHAIN")
            .is_none_or(|toolchain| toolchain.trim().is_empty())
    {
        return Err(RenderError::InvalidWorkflow(
            "mbx_observer_cache_identity_invalid".to_owned(),
        ));
    }
    let condition = pr_cache::QUALIFICATION_OBSERVER_CONDITION;
    let key = pr_cache::qualification_observer_key_step(
        MBX_BUNDLE_KEY_NAME,
        &generation,
        version,
        scope,
        rust_env,
        *binding,
    )?;
    let mut restore = restore_step(true)?;
    restore.condition = Some(condition.to_owned());
    let import = observer_import_step(condition)?;
    Ok(vec![key, restore, import])
}

fn observer_import_step(condition: &str) -> Result<Step, RenderError> {
    let mut env = BTreeMap::from([(
        "MATCHED".to_owned(),
        "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
    )]);
    let guard = identity::observer_import_guard(&mut env);
    let script = qualification::import_script(true, guard, IMPORT_SCRIPT);
    let mut step = crate::steps::shell_step(
        MBX_BUNDLE_IMPORT_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    step.condition = Some(condition.to_owned());
    Ok(step)
}
