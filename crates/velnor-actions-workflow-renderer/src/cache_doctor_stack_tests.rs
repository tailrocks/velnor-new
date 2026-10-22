use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::{Job, JobTimeout, Step, StepKind};

use crate::cache_steps::{
    FamilyCachePlan, FamilySnapshot, family_keys_step, family_restore_step, family_save_step,
    family_size_step,
};

fn plan(family: CacheFamily, locator: Option<&str>) -> FamilyCachePlan {
    FamilyCachePlan {
        family,
        locator: locator.map(str::to_owned),
        snapshot: FamilySnapshot::Files(vec!["lock".to_owned()]),
        compat: Vec::new(),
    }
}

fn job(steps: Vec<Step>) -> BTreeMap<String, Job> {
    BTreeMap::from([(
        "bun-web".to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Bun / web".to_owned(),
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

fn full(plan: &FamilyCachePlan) -> Vec<Step> {
    vec![
        family_keys_step(std::slice::from_ref(plan)).expect("keys"),
        family_restore_step(plan).expect("restore"),
        family_size_step(plan).expect("size"),
        family_save_step(plan).expect("save"),
    ]
}

fn with_mut(step: &mut Step) -> &mut BTreeMap<String, String> {
    let StepKind::Action { with, .. } = &mut step.kind else {
        panic!("action");
    };
    with
}

#[test]
fn the_doctor_admits_every_generated_family_step() {
    for plan in [
        plan(CacheFamily::BunInstall, None),
        plan(CacheFamily::PlaywrightBrowsers, None),
        plan(CacheFamily::DbtPackages, Some("analytics")),
        plan(CacheFamily::PythonVenv, None),
        plan(CacheFamily::AnsibleCollections, None),
    ] {
        crate::cache_doctor::check(&job(full(&plan)))
            .unwrap_or_else(|err| panic!("{:?}: {err}", plan.family));
    }
}

#[test]
fn the_doctor_rejects_a_foreign_path_a_loose_condition_and_a_hand_written_step() {
    let bun = plan(CacheFamily::BunInstall, None);
    let mut steps = full(&bun);
    with_mut(&mut steps[3]).insert(
        "path".to_owned(),
        "${{ runner.temp }}/velnor/other".to_owned(),
    );
    assert!(
        crate::cache_doctor::check(&job(steps)).is_err(),
        "foreign path"
    );

    let mut steps = full(&bun);
    steps[3].condition = Some("success()".to_owned());
    assert!(
        crate::cache_doctor::check(&job(steps)).is_err(),
        "ungated save"
    );

    let mut steps = full(&bun);
    with_mut(&mut steps[1]).insert("restore-keys".to_owned(), String::new());
    assert!(
        crate::cache_doctor::check(&job(steps)).is_err(),
        "restore keys"
    );

    let dbt = plan(CacheFamily::DbtPackages, Some("analytics"));
    let mut steps = full(&dbt);
    with_mut(&mut steps[3]).insert(
        "path".to_owned(),
        "${{ runner.temp }}/velnor/dbt-packages/../x".to_owned(),
    );
    assert!(
        crate::cache_doctor::check(&job(steps)).is_err(),
        "traversal"
    );

    let mut steps = full(&bun);
    with_mut(&mut steps[2 + 1]).insert("key".to_owned(), "velnor-v1-bun-install-hand".to_owned());
    assert!(
        crate::cache_doctor::check(&job(steps)).is_err(),
        "hand-written key"
    );
}

fn env_mut(step: &mut Step) -> &mut BTreeMap<String, String> {
    let StepKind::Shell { env, .. } = &mut step.kind else {
        panic!("shell");
    };
    env
}

/// A leg matrix job: the single-writer key step plus a marked script step.
fn leg_job(plan: &FamilyCachePlan) -> Vec<Step> {
    let mut steps = vec![
        crate::cache_steps::family_keys_step_single_writer(std::slice::from_ref(plan))
            .expect("keys"),
        family_restore_step(plan).expect("restore"),
        family_size_step(plan).expect("size"),
        family_save_step(plan).expect("save"),
    ];
    let mut script =
        crate::shell_step("Script e2e", vec!["true".to_owned()], BTreeMap::new()).expect("step");
    env_mut(&mut script).insert(
        crate::matrix::bun::BUN_LEGS_ENV.to_owned(),
        "1//1/2,2//2/2".to_owned(),
    );
    steps.push(script);
    steps
}

#[test]
fn the_doctor_admits_the_single_writer_key_step_in_a_leg_matrix_only() {
    let plan = plan(CacheFamily::PlaywrightBrowsers, None);
    crate::cache_doctor::check(&job(leg_job(&plan))).expect("leg matrix");
    // The writer env outside a leg matrix could only narrow writers, but the
    // doctor admits exactly the generated shapes.
    let mut outside = full(&plan);
    env_mut(&mut outside[0]).insert(
        "VELNOR_FAMILY_WRITER".to_owned(),
        "${{ matrix.leg }}".to_owned(),
    );
    let error = crate::cache_doctor::check(&job(outside)).expect_err("not a leg job");
    assert!(
        error
            .to_string()
            .contains("family_keys_env:VELNOR_FAMILY_WRITER"),
        "{error}"
    );
    let mut missing = leg_job(&plan);
    missing[0] =
        family_keys_step(std::slice::from_ref(&plan)).expect("keys without the writer guard");
    let error = crate::cache_doctor::check(&job(missing)).expect_err("every leg would save");
    assert!(
        error
            .to_string()
            .contains("family_keys_single_writer_missing"),
        "{error}"
    );
}

#[test]
fn the_doctor_rejects_any_other_key_step_env() {
    let plan = plan(CacheFamily::BunInstall, None);
    let mut other = full(&plan);
    env_mut(&mut other[0]).insert("VELNOR_TRUST".to_owned(), "trusted".to_owned());
    let error = crate::cache_doctor::check(&job(other)).expect_err("new env key");
    assert!(
        error.to_string().contains("family_keys_env:VELNOR_TRUST"),
        "{error}"
    );
    let mut value = leg_job(&plan);
    env_mut(&mut value[0]).insert("VELNOR_FAMILY_WRITER".to_owned(), "1".to_owned());
    let error = crate::cache_doctor::check(&job(value)).expect_err("literal writer");
    assert!(
        error
            .to_string()
            .contains("family_keys_env:VELNOR_FAMILY_WRITER"),
        "{error}"
    );
    let mut save = leg_job(&plan);
    save[3].condition = Some("always()".to_owned());
    let error = crate::cache_doctor::check(&job(save)).expect_err("changed save condition");
    assert!(
        error.to_string().contains("i7_save_not_trust_gated"),
        "{error}"
    );
}

#[test]
fn the_single_writer_step_differs_only_by_the_guard_and_its_env() {
    let plan = plan(CacheFamily::BunInstall, None);
    let base = family_keys_step(std::slice::from_ref(&plan)).expect("keys");
    let leg = crate::cache_steps::family_keys_step_single_writer(std::slice::from_ref(&plan))
        .expect("keys");
    let (
        StepKind::Shell {
            run: base_run,
            env: base_env,
        },
        StepKind::Shell {
            run: leg_run,
            env: leg_env,
        },
    ) = (&base.kind, &leg.kind)
    else {
        panic!("shell");
    };
    let guard = "case \"${VELNOR_FAMILY_WRITER:-}\" in 1) ;; *) save=false ;; esac; ";
    assert_eq!(
        base_run[2].replace(
            "printf '%s\\n' \"save=",
            &format!("{guard}printf '%s\\n' \"save=")
        ),
        leg_run[2]
    );
    assert_eq!(base_env.len() + 1, leg_env.len());
    assert_eq!(base.id, leg.id);
    assert_eq!(base.role, leg.role);
}
