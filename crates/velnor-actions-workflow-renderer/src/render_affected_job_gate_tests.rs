use super::*;

fn job(needs: Vec<String>, condition: Option<&str>) -> Job {
    Job {
        display_name: "Rust / crate".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: velnor_actions_contract::JobTimeout::CRATE,
        needs,
        condition: condition.map(str::to_owned),
        permissions: None,
        cache_mode: None,
        environment: None,
        check_runner: None,
        steps: Vec::new(),
    }
}

#[test]
fn crate_jobs_depend_on_the_plan_and_keep_existing_conditions() {
    let mut jobs = BTreeMap::from([
        (PLAN_JOB_ID.to_owned(), job(Vec::new(), None)),
        (
            "rust-demo".to_owned(),
            job(vec!["prepare".to_owned()], Some("always()")),
        ),
        ("required".to_owned(), job(Vec::new(), None)),
    ]);

    apply_affected_job_gates(&mut jobs).expect("valid jobs");

    let crate_job = &jobs["rust-demo"];
    assert_eq!(crate_job.needs, ["prepare", PLAN_JOB_ID]);
    assert_eq!(
        crate_job.condition.as_deref(),
        Some("(always()) && !contains(needs.plan.outputs.not_affected_jobs, ',rust-demo,')")
    );
    assert_eq!(jobs["required"].needs, Vec::<String>::new());
    assert!(jobs["required"].condition.is_none());
}

#[test]
fn crate_jobs_run_when_the_workflow_has_no_plan_job() {
    let mut jobs = BTreeMap::from([("rust-demo".to_owned(), job(Vec::new(), None))]);

    apply_affected_job_gates(&mut jobs).expect("valid jobs");

    assert_eq!(jobs["rust-demo"].needs, Vec::<String>::new());
    assert!(jobs["rust-demo"].condition.is_none());
}
