#[test]
fn plan_validates_sorting_and_matrix() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(3, 1);
    let entry = sample_entry(&run_key)?;
    let plan = Plan {
        schema: 1,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key)?,
        base: None,
        head: "ab".repeat(20),
        event: WorkflowEvent::PullRequest,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("no_entry"))?,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "ab".repeat(32),
        },
        packages: vec![PlanPackage {
            package_id: "demo 0.1.0".to_owned(),
            name: "demo".to_owned(),
            manifest: MANIFEST.to_owned(),
            selected: true,
            reasons: vec!["changed".to_owned()],
            tasks: vec![TASK.to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: TASK.to_owned(),
            decision: ObligationDecision::Execute,
            reason: "changed".to_owned(),
            task_digest: digest_b3(b"task"),
            input_digest: digest_b3(b"inputs"),
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![TASK.to_owned()],
        warnings: vec![],
        edges: vec![],
    };
    plan.validate()?;
    let mut bad = plan.clone();
    bad.task_ids = vec![TASK.to_owned(), "aaa".to_owned()];
    assert!(bad.validate().is_err());
    let covered = PlanObligation {
        decision: ObligationDecision::CoveredByTrustedBaseline,
        baseline_proof: Some(BaselineProof::new(
            &"ab".repeat(20),
            99,
            4242,
            &artifact_id_for_plan(&run_key)?,
            &digest_b3(b"manifest"),
        )?),
        ..plan.obligations[0].clone()
    };
    let mut plan2 = plan.clone();
    plan2.obligations = vec![covered];
    plan2.validate()?;
    Ok(())
}

#[test]
fn task_and_matrix_reports_validate() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(11, 1);
    let entry = sample_entry(&run_key)?;
    let task_digest = digest_b3(b"task-bytes");
    let report = TaskReport {
        schema: 1,
        task_report_id: task_report_id_for_task(&run_key, &entry.matrix_key, &task_digest)?,
        run_key: run_key.clone(),
        event: WorkflowEvent::PullRequest,
        trust: Trust::Pr,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: TASK.to_owned(),
        task_digest,
        status: TaskStatus::Executed,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: "velnor-v1-task-pr-x".to_owned(),
            result: CacheResult::Miss,
            miss_reason: Some("no_entry".to_owned()),
        },
        exit_code: 0,
        duration_ms: Some(12),
        outputs: vec![],
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    };
    report.validate()?;
    let skipped = TaskReport {
        status: TaskStatus::NotSelected,
        not_selected_reason: Some(NotSelectedReason::UpstreamFailed),
        cache: CacheOutcome {
            layer: CacheLayer::Task,
            key: "k".to_owned(),
            result: CacheResult::NotAttempted,
            miss_reason: None,
        },
        ..report.clone()
    };
    skipped.validate()?;
    let matrix = MatrixReport {
        schema: 1,
        report_id: entry.report_id.clone(),
        run_key,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        status: MatrixStatus::Passed,
        expected_task_ids: vec![TASK.to_owned()],
        task_report_ids: vec![report.task_report_id.clone()],
        tasks: vec![velnor_actions_contract::MatrixTaskEntry {
            task_report_id: report.task_report_id.clone(),
            task_id: TASK.to_owned(),
            status: TaskStatus::Executed,
            exit_code: 0,
        }],
        selected: 1,
        reused: 0,
        executed: 1,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    };
    matrix.validate()?;
    Ok(())
}
