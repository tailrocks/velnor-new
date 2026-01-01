//! Adapter-wire cases: plan edges, new events, entry outputs, miss reasons,
//! schedule placement, and timing breakdown (F2O contract halves).
use crate::impl_contract_ids::{GROUP, TASK, sample_entry};
use crate::impl_remed_contract::sample_plan;
use velnor_actions_contract::{
    ContractError, EdgeKind, ExecuteTaskRef, FinalCounts, FinalReport, FinalStatus, JobConclusion,
    PlatformBinding, PlatformRunnerEnvironment, PlatformUnavailableReason, RequiredJobResult,
    TaskEdge, TaskReport, TaskStatus, TaskTiming, WorkflowEvent, artifact_id_for_matrix,
    artifact_id_for_plan, digest_b3, final_report_id_for_run, plan_id_for_run, run_key_for_ci,
    task_report_id_for_task, validate_plan_edges,
};

#[test]
fn workflow_event_local_and_fork_roundtrip() {
    for (event, name) in [
        (WorkflowEvent::Local, "local"),
        (WorkflowEvent::Fork, "fork"),
    ] {
        let text = serde_json::to_string(&event).expect("serialize");
        assert_eq!(text, format!("\"{name}\""));
        let back: WorkflowEvent = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, event);
    }
    let legacy: WorkflowEvent = serde_json::from_str("\"pull_request\"").expect("legacy");
    assert_eq!(legacy, WorkflowEvent::PullRequest);
    let back: WorkflowEvent = serde_json::from_str("\"merge_group\"").expect("legacy");
    assert_eq!(back, WorkflowEvent::MergeGroup);
}

#[test]
fn plan_edges_validate_against_obligations() {
    let ids = vec![TASK.to_owned(), GROUP.to_owned()];
    let edge = TaskEdge {
        from: TASK.to_owned(),
        to: GROUP.to_owned(),
        kind: EdgeKind::ResourceExclusion,
    };
    assert_eq!(
        validate_plan_edges(std::slice::from_ref(&edge), &ids),
        Ok(())
    );
    assert_eq!(validate_plan_edges(&[], &ids), Ok(()));
    let looped = TaskEdge {
        from: TASK.to_owned(),
        to: TASK.to_owned(),
        kind: EdgeKind::Gate,
    };
    assert!(validate_plan_edges(std::slice::from_ref(&looped), &ids).is_err());
    let unknown = TaskEdge {
        from: TASK.to_owned(),
        to: "stack/rust/crates/missing/test/default".to_owned(),
        kind: EdgeKind::Data,
    };
    assert!(validate_plan_edges(std::slice::from_ref(&unknown), &ids).is_err());
    assert!(validate_plan_edges(&[edge.clone(), edge], &ids).is_err());
}

#[test]
fn plan_carries_edges_through_validate() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(31, 1);
    let mut plan = sample_plan(&run_key)?;
    plan.validate()?;
    plan.edges = vec![TaskEdge {
        from: TASK.to_owned(),
        to: TASK.to_owned(),
        kind: EdgeKind::Gate,
    }];
    assert!(plan.validate().is_err());
    plan.edges = vec![];
    plan.validate()?;
    Ok(())
}

#[test]
fn matrix_entry_carries_declared_outputs_and_test_run() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(32, 1);
    let mut entry = sample_entry(&run_key)?;
    assert_eq!(entry.declared_outputs, [] as [std::string::String; 0]);
    assert!(entry.test_run.is_empty());
    entry.validate(&run_key)?;
    entry.declared_outputs = vec!["target/report.json".to_owned()];
    entry.test_run = vec![
        ExecuteTaskRef::Single(TASK.to_owned()),
        ExecuteTaskRef::Shards(vec![TASK.to_owned(), GROUP.to_owned()]),
    ];
    entry.validate(&run_key)?;
    entry.declared_outputs = vec!["/abs/report.json".to_owned()];
    assert!(entry.validate(&run_key).is_err());
    entry.declared_outputs = vec!["target/report.json".to_owned()];
    entry.test_run = vec![ExecuteTaskRef::Shards(vec![])];
    assert!(entry.validate(&run_key).is_err());
    Ok(())
}

#[test]
fn final_report_miss_reasons_validate() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(33, 1);
    let entry = sample_entry(&run_key)?;
    let mut report = FinalReport {
        schema: 1,
        report_id: final_report_id_for_run(&run_key)?,
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
    report.validate()?;
    report.miss_reasons = vec![
        "cache_unavailable".to_owned(),
        "cache_write_disabled".to_owned(),
    ];
    report.validate()?;
    report.miss_reasons = vec!["sometimes".to_owned()];
    assert!(report.validate().is_err());
    Ok(())
}

#[test]
fn task_report_schedule_fields_validate() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(34, 1);
    let entry = sample_entry(&run_key)?;
    let task_digest = digest_b3(b"task-bytes");
    let timing = TaskTiming {
        queue_ms: 10,
        runner_ms: 20,
        task_ms: 30,
        cache_ms: 4,
        prep_ms: 5,
        download_ms: 6,
        compiler_ms: 70,
        mbx_ms: 8,
        test_ms: 90,
        lock_wait_ms: 1,
    };
    assert_eq!(timing.accounted_total(), 244);
    assert_eq!(timing.slots().len(), 10);
    let mut report = TaskReport {
        schema: TaskReport::SCHEMA,
        task_report_id: task_report_id_for_task(&run_key, &entry.matrix_key, &task_digest)?,
        run_key,
        event: WorkflowEvent::Local,
        trust: velnor_actions_contract::Trust::Pr,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: TASK.to_owned(),
        task_digest,
        status: TaskStatus::Executed,
        not_selected_reason: None,
        cache: velnor_actions_contract::CacheOutcome {
            layer: velnor_actions_contract::CacheLayer::Task,
            key: "k".to_owned(),
            result: velnor_actions_contract::CacheResult::Hit,
            miss_reason: None,
        },
        platform_binding: PlatformBinding::Unavailable {
            planned_platform_id: digest_b3(b"planned-platform"),
            runner_environment: PlatformRunnerEnvironment::Unknown,
            reason: PlatformUnavailableReason::ObservationNotRecorded,
        },
        exit_code: 0,
        duration_ms: Some(244),
        outputs: vec![],
        lane: Some(2),
        queue: Some("compiler-shared".to_owned()),
        partition: Some("hash:1/4".to_owned()),
        reason: Some("weight".to_owned()),
        timing: Some(timing),
    };
    report.validate()?;
    report.queue = Some("/abs/queue".to_owned());
    assert!(report.validate().is_err());
    report.queue = None;
    report.validate()?;
    Ok(())
}
