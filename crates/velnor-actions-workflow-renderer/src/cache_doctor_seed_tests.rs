//! Doctor rules for the dependency seed families.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, RSDEPS_HELPER_KEY_STEP_EXPR, RSDEPS_SAVE_CONDITION, key_step_expr,
    plan_output_expr, rsdeps_key_output, rsdeps_restore_keys_output,
};
use velnor_actions_contract::{Job, JobTimeout, Step, StepKind};

use super::super::check;
use super::DoctorScope;
use crate::cache_steps::{SeedCacheUse, seed_cache_step};

const RESTORE: &str = "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const SAVE: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";

fn jobs(steps: Vec<Step>) -> BTreeMap<String, Job> {
    BTreeMap::from([(
        "cache-seed".to_owned(),
        Job {
            display_name: "Cache seed".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps,
            cache_mode: None,
        },
    )])
}

fn step_key(n: usize) -> String {
    key_step_expr(&rsdeps_key_output(n))
}

fn reader(n: usize) -> Step {
    seed_cache_step(
        RESTORE,
        CacheFamily::Rsdeps,
        "nested",
        SeedCacheUse::Restore,
        &plan_output_expr(&rsdeps_key_output(n)),
        Some(&plan_output_expr(&rsdeps_restore_keys_output(n))),
    )
    .expect("reader")
}

fn save(condition: &str) -> Step {
    let mut step = seed_cache_step(
        SAVE,
        CacheFamily::Rsdeps,
        "",
        SeedCacheUse::Save,
        &step_key(0),
        None,
    )
    .expect("save");
    step.condition = Some(condition.to_owned());
    step
}

fn set(step: &mut Step, key: &str, value: &str) {
    let StepKind::Action { with, .. } = &mut step.kind else {
        panic!("action");
    };
    with.insert(key.to_owned(), value.to_owned());
}

#[test]
fn spec_lookups_restores_and_the_gated_seed_save_pass() {
    let lookup = seed_cache_step(
        RESTORE,
        CacheFamily::Rsdeps,
        "",
        SeedCacheUse::Lookup,
        &step_key(0),
        None,
    )
    .expect("lookup");
    let helper = seed_cache_step(
        RESTORE,
        CacheFamily::RsdepsHelper,
        "",
        SeedCacheUse::Lookup,
        RSDEPS_HELPER_KEY_STEP_EXPR,
        None,
    )
    .expect("helper");
    let all = jobs(vec![lookup, helper, reader(1), save(RSDEPS_SAVE_CONDITION)]);
    check(&all, DoctorScope::Seed).expect("seed workflow");
    check(&jobs(vec![reader(1)]), DoctorScope::Ci).expect("ci reader");
}

#[test]
fn dependency_seeds_are_saved_only_by_the_seed_workflow_under_its_gate() {
    let err =
        check(&jobs(vec![save(RSDEPS_SAVE_CONDITION)]), DoctorScope::Ci).expect_err("ci save");
    assert!(err.to_string().contains("seed_role_outside_seed"), "{err}");
    let mut untyped = save(RSDEPS_SAVE_CONDITION);
    untyped.role = None;
    let err = check(&jobs(vec![untyped]), DoctorScope::Ci).expect_err("untyped ci save");
    assert!(err.to_string().contains("i7_save_not_trust_gated"), "{err}");
    let err = check(&jobs(vec![save("success()")]), DoctorScope::Seed).expect_err("ungated");
    assert!(err.to_string().contains("i7_save_not_trust_gated"), "{err}");
    let mut plan_keyed = save(RSDEPS_SAVE_CONDITION);
    set(
        &mut plan_keyed,
        "key",
        &plan_output_expr(&rsdeps_key_output(0)),
    );
    let err = check(&jobs(vec![plan_keyed]), DoctorScope::Seed).expect_err("plan key");
    assert!(err.to_string().contains("i7_save_not_trust_gated"), "{err}");
}

#[test]
fn seed_only_roles_are_rejected_outside_the_seed_workflow() {
    let mut janitor = reader(1);
    janitor.role = Some(velnor_actions_contract::StepRole::SeedJanitor);
    let err = check(&jobs(vec![janitor]), DoctorScope::Ci).expect_err("ci janitor");
    assert!(err.to_string().contains("seed_role_outside_seed"), "{err}");
}

#[test]
fn paths_prefixes_and_lookups_must_come_from_one_root_spec() {
    for (key, value, rule) in [
        ("path", "nested/target/debug", "i2_path_not_from_spec"),
        (
            "path",
            "other/target/debug\nnested/target/velnor-clippy/debug\nnested/target/velnor-doc/debug",
            "i2_path_not_from_spec",
        ),
        (
            "restore-keys",
            "${{needs.plan.outputs.rsdeps_restore_keys_0}}",
            "i2_path_not_from_spec",
        ),
        ("lookup-only", "true", "i2_path_not_from_spec"),
    ] {
        let mut step = reader(1);
        set(&mut step, key, value);
        let err = check(&jobs(vec![step]), DoctorScope::Ci).expect_err(rule);
        assert!(err.to_string().contains(rule), "{key}: {err}");
    }
    let mut literal = reader(1);
    set(&mut literal, "key", "velnor-v2-rsdeps-trusted-r0-x");
    assert!(check(&jobs(vec![literal]), DoctorScope::Ci).is_err());
}

#[test]
fn a_seed_path_never_nests_another_family_path() {
    let mut sources = reader(1);
    set(&mut sources, "path", "nested/target/debug");
    let shared = super::super::check_disjoint(&BTreeMap::from([
        ("nested/target".to_owned(), super::super::Family::Tools),
        (
            "nested/target/debug".to_owned(),
            super::super::Family::Runtime(CacheFamily::Rsdeps),
        ),
    ]))
    .expect_err("nested");
    assert!(shared.to_string().contains("i3_path_shared"), "{shared}");
}
