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
