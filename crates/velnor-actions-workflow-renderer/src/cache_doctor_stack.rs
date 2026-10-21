//! Path and restore-key rules of the stack family caches (Bun, dbt, Ansible).

use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_steps::{
    family_restore_keys_expr, root_locator,
};
use velnor_actions_contract::{Step, StepKind};

/// A stack family archives exactly its spec path (per-root families under one safe locator).
pub(super) fn path_ok(family: CacheFamily, path: &str) -> bool {
    let spec = family.spec();
    if !spec.is_per_root() {
        return path == spec.path_input(None);
    }
    let (head, tail) = spec.paths[0]
        .split_once(velnor_actions_contract::workflow::cache_family::ROOT_PLACEHOLDER)
        .unwrap_or((spec.paths[0], ""));
    path.strip_prefix(head)
        .and_then(|rest| rest.strip_suffix(tail))
        .is_some_and(|locator| root_locator(locator).as_deref() == Some(locator))
}

/// A restore reads its family's same-compatibility prefix; a save names none.
pub(super) fn restore_keys_ok(step: &Step, family: CacheFamily, save: bool) -> bool {
    let StepKind::Action { with, .. } = &step.kind else {
        return false;
    };
    if save {
        !with.contains_key("restore-keys")
    } else {
        with.get("restore-keys") == Some(&family_restore_keys_expr(family))
    }
}

#[cfg(test)]
#[path = "cache_doctor_stack_tests.rs"]
mod tests;
