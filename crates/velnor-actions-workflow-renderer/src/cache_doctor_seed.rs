//! Cache doctor rules for the dependency seed (`rsdeps`) families.
//!
//! - Keys come only from the key step (`steps.cache-keys`) or the Plan
//!   outputs (`needs.plan`), never literals.
//! - Every lookup, restore, and save archives the family spec path list of
//!   one valid workspace root (identical across Plan, crate jobs, seed).
//! - Restores pair each key with the same root's restore prefix; lookups
//!   are `lookup-only`; saves carry no prefix.
//! - Only the Cache seed workflow saves, under the exact producer gate.

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, RSDEPS_HELPER_KEY_STEP_EXPR, RSDEPS_HELPER_RESTORE_KEYS_STEP_EXPR,
    RSDEPS_HELPER_SAVE_CONDITION, RSDEPS_KEY_OUTPUT_PREFIX, RSDEPS_RESTORE_KEYS_OUTPUT_PREFIX,
    RSDEPS_SAVE_CONDITION, output_ordinal,
};
use velnor_actions_contract::{Step, StepKind};

/// Which generated workflow the doctor is checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoctorScope {
    /// `ci.yml` and suite workflows: dependency seeds are read-only.
    Ci,
    /// The Cache seed workflow: the only dependency seed writer.
    Seed,
}

const STEP_SOURCE: &str = "steps.cache-keys.outputs.";
const PLAN_SOURCE: &str = "needs.plan.outputs.";

/// `(source, ordinal)` of an `rsdeps_key_<n>` reference.
fn key_reference(key: &str) -> Option<(&'static str, usize)> {
    let inner = key.strip_prefix("${{")?.strip_suffix("}}")?;
    [STEP_SOURCE, PLAN_SOURCE].into_iter().find_map(|source| {
        inner
            .strip_prefix(source)
            .and_then(|name| output_ordinal(name, RSDEPS_KEY_OUTPUT_PREFIX))
            .map(|ordinal| (source, ordinal))
    })
}

/// Seed family of a key expression, when it is one.
pub(crate) fn family_for_key(key: &str) -> Option<CacheFamily> {
    if key == RSDEPS_HELPER_KEY_STEP_EXPR {
        return Some(CacheFamily::RsdepsHelper);
    }
    key_reference(key)
        .filter(|(_, ordinal)| {
            *ordinal < velnor_actions_contract::workflow::cache_family::MAX_RSDEPS_ROOTS
        })
        .map(|_| CacheFamily::Rsdeps)
}

/// The restore prefix expression that pairs with `key`.
fn restore_keys_for(key: &str) -> Option<String> {
    if key == RSDEPS_HELPER_KEY_STEP_EXPR {
        return Some(RSDEPS_HELPER_RESTORE_KEYS_STEP_EXPR.to_owned());
    }
    key_reference(key).map(|(source, ordinal)| {
        format!("${{{{{source}{RSDEPS_RESTORE_KEYS_OUTPUT_PREFIX}{ordinal}}}}}")
    })
}

/// Workspace root named by a seed family path list, when it is the spec list.
fn root_of_paths(family: CacheFamily, path: &str) -> Option<String> {
    let first = path.lines().next()?;
    let prefix = first.strip_suffix(family.spec().paths[0].trim_start_matches("{root}"))?;
    let root = prefix.strip_suffix('/').unwrap_or(prefix);
    (prefix.is_empty() || prefix.ends_with('/')).then(|| root.to_owned())
}

/// I2 for one seed family step: spec paths, lookup flag, and paired prefix.
pub(crate) fn paths_ok(family: CacheFamily, step: &Step, path: &str, save: bool) -> bool {
    let StepKind::Action { with, .. } = &step.kind else {
        return false;
    };
    let Some(root) = root_of_paths(family, path) else {
        return false;
    };
    if crate::cache_steps::seed_path_input(family, &root)
        .ok()
        .as_deref()
        != Some(path)
    {
        return false;
    }
    let key = with.get("key").map(String::as_str).unwrap_or_default();
    let lookup = with.get("lookup-only").map(String::as_str);
    let prefix = with.get("restore-keys").map(String::as_str);
    match (save, lookup, prefix) {
        (true, None, None) | (false, Some("true"), None) => true,
        (false, None, Some(prefix)) => restore_keys_for(key).as_deref() == Some(prefix),
        _ => false,
    }
}

/// Seed-only roles (the janitor's token binding among them) never appear
/// outside the Cache seed workflow.
pub(crate) fn role_in_scope(step: &Step, scope: DoctorScope) -> bool {
    use velnor_actions_contract::StepRole;
    scope == DoctorScope::Seed
        || !matches!(
            step.role,
            Some(
                StepRole::SeedJanitor
                    | StepRole::SeedLane
                    | StepRole::SeedTrim
                    | StepRole::SeedSkeleton
                    | StepRole::DependencySeedSave
            )
        )
}

/// I7 for one seed family save: only the seed workflow, under the exact gate.
pub(crate) fn save_ok(family: CacheFamily, step: &Step, scope: DoctorScope) -> bool {
    let StepKind::Action { with, .. } = &step.kind else {
        return false;
    };
    let key = with.get("key").map(String::as_str).unwrap_or_default();
    let from_key_step = key == RSDEPS_HELPER_KEY_STEP_EXPR
        || key_reference(key).is_some_and(|(source, _)| source == STEP_SOURCE);
    let expected = match family {
        CacheFamily::RsdepsHelper => RSDEPS_HELPER_SAVE_CONDITION,
        _ => RSDEPS_SAVE_CONDITION,
    };
    scope == DoctorScope::Seed && from_key_step && step.condition.as_deref() == Some(expected)
}

#[cfg(test)]
#[path = "cache_doctor_seed_tests.rs"]
mod tests;
