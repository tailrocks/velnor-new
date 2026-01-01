//! Remediation cases: CACHE audit rows (reports, paths, extensions).
use crate::impl_contract_ids::{TASK, sample_entry, sample_identity};
use velnor_actions_contract::cachekey::{MISS_REASONS, cache_key, validate_miss_reason};
use velnor_actions_contract::{
    ContractError, StackExtension, digest_b3, input_digest, run_key_for_ci, task_report_id_for_task,
};
use velnor_actions_contract_release::{
    RUST_EXTENSION_REQUIRED_SLOTS, TOFU_EXTENSION_REQUIRED_SLOTS, validate_rust_extension,
    validate_tofu_extension,
};
use velnor_actions_contract_workflow::{
    CacheLayer, CacheOutcome, CacheResult, TaskReport, TaskStatus, Trust, WorkflowEvent,
    final_report_id_for_run, final_report_relpath, join_runner_temp, matrix_report_relpath,
    task_report_relpath,
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
fn cache_tofu_providers_layer_reports_and_roundtrips() -> Result<(), ContractError> {
    let compat = digest_b3(b"compat");
    let snapshot = digest_b3(b"snapshot");
    let key = cache_key("tofu-providers", "trusted", &compat, &snapshot)?;
    let wire = format!(r#"{{"layer":"tofu-providers","key":"{key}","result":"hit"}}"#);
    let outcome: CacheOutcome = serde_json::from_str(&wire).expect("provider layer parses");
    assert_eq!(outcome.result, CacheResult::Hit);
    let back = serde_json::to_string(&outcome).expect("serialize");
    assert!(back.contains("\"layer\":\"tofu-providers\""), "{back}");
    Ok(())
}

#[test]
fn cache_provider_compatibility_binds_dimensions_and_trust_scopes_keys() -> Result<(), ContractError>
{
    use velnor_actions_contract::{CompatibilityInputs, compatibility_id};
    let inputs = CompatibilityInputs {
        schema_id: "v1".to_owned(),
        repository_id: digest_b3(b"repo"),
        workspace_id: digest_b3(b"workspace"),
        lane_id: digest_b3(b"lane"),
        platform_id: digest_b3(b"linux-x86_64"),
        toolchain_id: digest_b3(b"opentofu-1.13.1"),
        cache_format_id: digest_b3(b"tofu-format"),
        stack_extension_id: digest_b3(b"lock-digest"),
    };
    let compat = compatibility_id(&inputs)?;
    let mut drifted = inputs.clone();
    drifted.stack_extension_id = digest_b3(b"changed-lock");
    assert_ne!(
        compatibility_id(&drifted)?,
        compat,
        "a lock change flips provider compatibility"
    );
    let snapshot = digest_b3(b"snapshot");
    let trusted = cache_key("tofu-providers", "trusted", &compat, &snapshot)?;
    let pr = cache_key("tofu-providers", "pr", &compat, &snapshot)?;
    assert_ne!(trusted, pr, "trust namespaces the key");
    assert!(
        trusted.contains("-trusted-") && pr.contains("-pr-"),
        "trust rides the key, never the digest"
    );
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

#[test]
fn tofu_extension_slots_require_unit_graph_and_driver() {
    assert_eq!(TOFU_EXTENSION_REQUIRED_SLOTS.len(), 9);
    let good = StackExtension {
        schema: "tofu-task-identity-v1".to_owned(),
        data: serde_json::json!({
            "unit_id": "root",
            "workspace_id": digest_b3(b"workspace"),
            "graph_digest": digest_b3(b"graph"),
            "root": "",
            "profile": "default",
            "driver": "tofu+none",
            "config_digest": digest_b3(b"config"),
            "lock_digest": null,
            "kind": "validate",
        }),
    };
    assert_eq!(validate_tofu_extension(&good), Ok(()));
    let mut nested = good.clone();
    nested.data["unit_id"] = serde_json::json!("stacks/a");
    nested.data["root"] = serde_json::json!("stacks/a");
    nested.data["lock_digest"] = serde_json::json!(digest_b3(b"lock"));
    assert_eq!(validate_tofu_extension(&nested), Ok(()));
    let mut unknown = good.clone();
    unknown.schema = "rust-task-identity-v1".to_owned();
    assert!(validate_tofu_extension(&unknown).is_err());
    for slot in ["unit_id", "graph_digest", "driver", "lock_digest"] {
        let mut value = serde_json::to_value(&good.data).expect("value");
        value.as_object_mut().expect("object").remove(slot);
        let missing = StackExtension {
            schema: good.schema.clone(),
            data: value,
        };
        let err = validate_tofu_extension(&missing).expect_err("missing slot");
        assert!(
            err.to_string().contains(&format!("missing_slot:{slot}")),
            "{slot}"
        );
    }
    let mut bad = good.clone();
    bad.data["driver"] = serde_json::json!("tofu");
    assert!(validate_tofu_extension(&bad).is_err());
    let mut bad = good.clone();
    bad.data["config_digest"] = serde_json::json!("nope");
    assert!(validate_tofu_extension(&bad).is_err());
    let mut bad = good;
    bad.data["lock_digest"] = serde_json::json!(42);
    assert!(validate_tofu_extension(&bad).is_err());
}

#[test]
fn cache_provider_layer_reports_only_closed_reasons() -> Result<(), ContractError> {
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
            layer: CacheLayer::TofuProviders,
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
    assert_eq!(MISS_REASONS.len(), 13);
    for reason in MISS_REASONS {
        report.cache.miss_reason = Some((*reason).to_owned());
        report.validate()?;
    }
    report.cache.miss_reason = Some("sometimes".to_owned());
    assert!(report.validate().is_err());
    Ok(())
}
