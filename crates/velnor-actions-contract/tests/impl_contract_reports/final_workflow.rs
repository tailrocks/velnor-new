#[test]
fn final_reports_validate() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(5, 3);
    let entry = sample_entry(&run_key)?;
    let report_id = final_report_id_for_run(&run_key)?;
    validate_final_report_id(&report_id)?;
    let final_report = FinalReport {
        schema: 1,
        report_id,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key)?,
        expected_report_ids: vec![entry.report_id.clone()],
        downloaded_artifact_ids: vec![
            artifact_id_for_matrix(&run_key, &entry.matrix_key)?,
            artifact_id_for_plan(&run_key)?,
        ],
        required_job_results: vec![RequiredJobResult {
            job_id: "plan".to_owned(),
            conclusion: JobConclusion::Success,
        }],
        status: FinalStatus::Passed,
        counts: FinalCounts {
            selected: 1,
            reused: 0,
            executed: 1,
            empty_partition: 0,
            covered: 0,
            failed: 0,
            cancelled: 0,
            blocked: 0,
            not_run: 0,
        },
        miss_reasons: vec![],
    };
    final_report.validate()?;
    assert_eq!(
        final_report.artifact_id()?,
        format!("velnor-final-{run_key}")
    );
    Ok(())
}

#[test]
fn workflow_ir_validates_pins_and_refs() -> Result<(), ContractError> {
    let job = Job {
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::PLAN,
        needs: vec![],
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![Step {
            name: "Checkout".to_owned(),
            condition: None,
            kind: StepKind::Shell {
                run: vec!["mise".to_owned(), "install".to_owned()],
                env: BTreeMap::new(),
            },
        }],
    };
    let ir = WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "velnor-ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs: BTreeMap::from([("plan".to_owned(), job)]),
    };
    ir.validate()?;
    let mut bad = ir.clone();
    if let Some(job) = bad.jobs.get_mut("plan") {
        job.runs_on = "ubuntu-latest".to_owned();
    }
    assert!(bad.validate().is_err());
    for display in ["Plan ${{ secrets.x }}", "Plan\nInjected: true", "Plan\tx"] {
        let mut bad = ir.clone();
        if let Some(job) = bad.jobs.get_mut("plan") {
            job.display_name = display.to_owned();
        }
        let err = bad.validate().expect_err("display must fail closed");
        assert!(
            err.to_string().contains("bad_display_name"),
            "{display:?}: {err}"
        );
    }
    Ok(())
}
