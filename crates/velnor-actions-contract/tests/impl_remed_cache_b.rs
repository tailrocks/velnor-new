//! Remediation cases: CACHE audit rows (reports, paths, extensions).
use crate::impl_contract_ids::{TASK, sample_entry, sample_identity};
use velnor_actions_contract::cachekey::{MISS_REASONS, cache_key, validate_miss_reason};
use velnor_actions_contract::{
    CacheLayer, CacheOutcome, CacheResult, ContractError, RUST_EXTENSION_REQUIRED_SLOTS,
    StackExtension, TaskReport, TaskStatus, Trust, WorkflowEvent, digest_b3,
    final_report_id_for_run, final_report_relpath, input_digest, join_runner_temp,
    matrix_report_relpath, run_key_for_ci, task_report_id_for_task, task_report_relpath,
    validate_rust_extension,
};

#[test]
fn cache_commit_sha_alone_is_not_identity() -> Result<(), ContractError> {
    let sha = "ab".repeat(20);
    assert!(cache_key("task", "trusted", &sha, &digest_b3(b"s")).is_err());
    let mut first = sample_identity();
    first.vcs.commit = Some(sha.clone());
    let mut second = sample_identity();
    second.vcs.commit = Some(sha);
    second.argv.push("--extra".to_owned());
    assert_ne!(input_digest(&first)?, input_digest(&second)?);
    assert_eq!(input_digest(&first)?, input_digest(&first)?);
    Ok(())
}

#[test]
fn cache_miss_reason_membership_enforced() -> Result<(), ContractError> {
    assert_eq!(MISS_REASONS.len(), 13);
    for reason in MISS_REASONS {
        assert_eq!(validate_miss_reason(reason), Ok(()));
    }
    assert!(validate_miss_reason("sometimes").is_err());
    let run_key = run_key_for_ci(22, 1);
    let entry = sample_entry(&run_key)?;
    let task_digest = digest_b3(b"task-bytes");
    let mut report = TaskReport {
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
            key: "k".to_owned(),
            result: CacheResult::Miss,
            miss_reason: Some("no_entry".to_owned()),
        },
        exit_code: 0,
        duration_ms: Some(1),
        outputs: vec![],
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    };
    report.validate()?;
    report.cache.miss_reason = Some("sometimes".to_owned());
    assert!(report.validate().is_err());
    Ok(())
}

#[test]
fn cache_report_outputs_declared_and_secret_free() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(23, 1);
    let entry = sample_entry(&run_key)?;
    let task_digest = digest_b3(b"task-bytes");
    let mut report = TaskReport {
        schema: 1,
        task_report_id: task_report_id_for_task(&run_key, &entry.matrix_key, &task_digest)?,
        run_key,
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
            key: "k".to_owned(),
            result: CacheResult::Hit,
            miss_reason: None,
        },
        exit_code: 0,
        duration_ms: Some(1),
        outputs: vec!["target/report.json".to_owned()],
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    };
    report.validate()?;
    let declared = vec!["target/report.json".to_owned()];
    assert_eq!(report.validate_outputs_declared(&declared), Ok(()));
    assert!(report.validate_outputs_declared(&[]).is_err());
    report.outputs = vec!["/abs/report.json".to_owned()];
    assert!(report.validate().is_err());
    let text = serde_json::to_string(&report).expect("serialize");
    for key in ["environment", "secret", "token", "credential"] {
        assert!(!text.contains(key), "leaked {key}");
    }
    Ok(())
}

#[test]
fn cache_final_report_path_and_ids() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(24, 1);
    let entry = sample_entry(&run_key)?;
    assert_eq!(
        final_report_id_for_run(&run_key)?,
        format!("final-{run_key}")
    );
    assert_eq!(
        final_report_relpath(&run_key)?,
        format!("velnor/{run_key}/final-report.json")
    );
    assert_eq!(
        matrix_report_relpath(&run_key, &entry.matrix_key)?,
        format!("velnor/{run_key}/{}/matrix-report.json", entry.matrix_key)
    );
    let task_digest = digest_b3(b"t");
    let task_report = task_report_id_for_task(&run_key, &entry.matrix_key, &task_digest)?;
    assert_eq!(
        task_report_relpath(&run_key, &entry.matrix_key, &task_report)?,
        format!(
            "velnor/{run_key}/{}/tasks/{task_report}.json",
            entry.matrix_key
        )
    );
    assert!(final_report_relpath("bogus").is_err());
    let relpath = final_report_relpath(&run_key)?;
    assert_eq!(
        join_runner_temp("/tmp/runner", &relpath)?,
        format!("/tmp/runner/velnor/{run_key}/final-report.json")
    );
    assert_eq!(
        join_runner_temp("/tmp/runner/", &relpath)?,
        format!("/tmp/runner/velnor/{run_key}/final-report.json")
    );
    assert!(join_runner_temp("", &relpath).is_err());
    assert!(join_runner_temp("/tmp/runner", "/abs/report.json").is_err());
    assert!(join_runner_temp("/tmp/runner", "velnor/../escape.json").is_err());
    Ok(())
}

#[test]
fn cache_extension_slots_require_workspace_and_profile() {
    assert_eq!(RUST_EXTENSION_REQUIRED_SLOTS.len(), 11);
    let good = StackExtension {
        schema: "rust-task-identity-v1".to_owned(),
        data: serde_json::json!({
            "package_id": "demo 0.1.0",
            "workspace_id": velnor_actions_contract::digest_b3(b"workspace"),
            "graph_digest": velnor_actions_contract::digest_b3(b"graph"),
            "targets": ["lib"],
            "features": ["default"],
            "profile": "test",
            "driver": "cargo+cargo_nextest",
            "config_digest": velnor_actions_contract::digest_b3(b"config"),
            "nextest_digest": velnor_actions_contract::digest_b3(b"nextest"),
            "kind": "nextest",
            "archive": null,
        }),
    };
    assert_eq!(validate_rust_extension(&good), Ok(()));
    let mut unknown = good.clone();
    unknown.schema = "rust-task-v2".to_owned();
    assert!(validate_rust_extension(&unknown).is_err());
    for slot in ["workspace_id", "profile", "graph_digest", "driver"] {
        let mut value = serde_json::to_value(&good.data).expect("value");
        value.as_object_mut().expect("object").remove(slot);
        let missing = StackExtension {
            schema: good.schema.clone(),
            data: value,
        };
        let err = validate_rust_extension(&missing).expect_err("missing slot");
        assert!(
            err.to_string().contains(&format!("missing_slot:{slot}")),
            "{slot}"
        );
    }
    let mut bad = good.clone();
    bad.data["driver"] = serde_json::json!("cargo");
    assert!(validate_rust_extension(&bad).is_err());
    let mut bad = good;
    bad.data["graph_digest"] = serde_json::json!("nope");
    assert!(validate_rust_extension(&bad).is_err());
}
