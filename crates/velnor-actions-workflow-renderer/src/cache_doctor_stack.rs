use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_steps::{
    FAMILY_WRITER_ENV, FAMILY_WRITER_EXPR,
};
use velnor_actions_contract::workflow::cache_family_steps::{
    family_restore_keys_expr, root_locator,
};
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

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

pub(super) fn check_family_keys_env(id: &str, job: &Job, step: &Step) -> Result<(), String> {
    if step.role != Some(StepRole::FamilyKeys) {
        return Ok(());
    }
    let StepKind::Shell { env, .. } = &step.kind else {
        return Err("family_keys_not_shell".to_owned());
    };
    let leg_job = crate::matrix::bun::has_bun_matrix(job);
    let scrub = crate::toolchain_env::credential_scrub();
    for (key, value) in env {
        let ok = match key.as_str() {
            "VELNOR_DEFAULT_REF" => true,
            FAMILY_WRITER_ENV => leg_job && value == FAMILY_WRITER_EXPR,
            other => scrub.get(other) == Some(value),
        };
        if !ok {
            return Err(format!("family_keys_env:{key}"));
        }
    }
    if leg_job && !env.contains_key(FAMILY_WRITER_ENV) {
        return Err(format!("family_keys_single_writer_missing:{id}"));
    }
    Ok(())
}
