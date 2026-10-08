use velnor_actions_orchestrator_merge_ports::{
    BaselineManifest, MergeRequest, SCHEMA, Signals, TaskReportOutputFanIn, TestIdentity,
    check_schema, inventory_digest,
};

#[test]
fn schema_constant_is_one() {
    assert_eq!(SCHEMA, 1);
}

#[test]
fn check_schema_accepts_one_only() {
    assert!(check_schema(1).is_ok());
    for schema in [0, 2, 99, u32::MAX] {
        let err = check_schema(schema).expect_err("schema must reject");
        assert!(
            err.to_string().contains("unsupported_schema"),
            "unexpected: {err}"
        );
    }
}

#[test]
fn signals_default_all_clear() {
    let signals = Signals::default();
    assert!(
        !signals.planning_failed
            && !signals.failed
            && !signals.cancelled
            && !signals.blocked
            && !signals.not_run
    );
}

fn identity(package: &str, features: &[&str]) -> TestIdentity {
    TestIdentity {
        package: package.to_owned(),
        target: "lib".to_owned(),
        features: features.iter().map(ToString::to_string).collect(),
        binary: "bin".to_owned(),
        name: "case".to_owned(),
    }
}

#[test]
fn test_identity_accepts_clean_sorted() {
    assert!(identity("pkg", &["a", "b"]).validate().is_ok());
}

#[test]
fn test_identity_rejects_malformed() {
    assert!(identity("", &[]).validate().is_err());
    assert!(identity("/abs", &[]).validate().is_err());
    assert!(identity("pkg", &["b", "a"]).validate().is_err());
}

#[test]
fn inventory_digest_is_order_independent() {
    let left = identity("a", &[]);
    let right = identity("b", &[]);
    assert_eq!(
        inventory_digest(&[left.clone(), right.clone()]),
        inventory_digest(&[right, left])
    );
}

#[test]
fn inventory_digest_is_deterministic_and_nonempty() {
    let tests = [identity("a", &["x"])];
    let digest = inventory_digest(&tests);
    assert!(!digest.is_empty());
    assert_eq!(digest, inventory_digest(&tests));
}

#[test]
fn merge_request_parses_minimal_shape() {
    let request: MergeRequest = serde_json::from_value(serde_json::json!({
        "schema": 1,
        "run_key": "run",
        "matrix_reports": [],
        "required_job_ids": [],
        "required_jobs": [],
    }))
    .expect("minimal request");
    assert_eq!(request.schema, 1);
    assert_eq!(request.run_key, "run");
    assert!(request.plan.is_none());
    assert!(request.baseline_manifest.is_none());
    assert!(request.task_report_outputs.is_none());
}

#[test]
fn merge_request_rejects_present_null_report_output_sidecar() {
    let parsed = serde_json::from_value::<MergeRequest>(serde_json::json!({
        "schema": 1,
        "run_key": "run",
        "matrix_reports": [],
        "required_job_ids": [],
        "required_jobs": [],
        "task_report_outputs": null,
    }));
    assert!(parsed.is_err());
}

fn task_report_outputs() -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "origin": "github_com",
        "run": {
            "repository_id": "123",
            "repository": "owner/repository",
            "run_id": "456",
            "run_attempt": 2,
        },
        "head_sha": "0123456789abcdef",
        "plan_digest": "abcdef0123456789",
        "expected_workflow_job_keys": ["task-linux", "task-macos"],
        "producers": [
            {
                "workflow_job_key": "task-linux",
                "conclusion": "success",
                "artifact_id": 77,
                "check_run_id": 88,
            },
            {
                "workflow_job_key": "task-macos",
                "conclusion": "success",
                "artifact_id": 77,
                "check_run_id": 88,
            },
        ],
    })
}

#[test]
fn task_report_output_fanin_parses_typed_distinct_id_namespaces() {
    let parsed: TaskReportOutputFanIn =
        serde_json::from_value(task_report_outputs()).expect("valid sidecar");
    assert_eq!(parsed.producers.len(), 2);
    assert_eq!(parsed.producers[0].artifact_id.get(), 77);
    assert_eq!(parsed.producers[0].check_run_id.get(), 88);
    // Repeated numeric values remain available for the later authoritative
    // provider join; this parser does not guess whether they are duplicates.
    assert_eq!(parsed.producers[1].artifact_id.get(), 77);
    assert_eq!(parsed.producers[1].check_run_id.get(), 88);
}

#[test]
fn task_report_output_fanin_rejects_invalid_expected_producer_sets() {
    assert_sidecar_rejected("missing producer", |value| {
        value["producers"] = serde_json::json!([]);
    });
    assert_sidecar_rejected("foreign producer", |value| {
        value["producers"][1]["workflow_job_key"] = serde_json::json!("other-job");
    });
    assert_sidecar_rejected("duplicate expected key", |value| {
        value["expected_workflow_job_keys"][1] = serde_json::json!("task-linux");
    });
    assert_sidecar_rejected("duplicate producer key", |value| {
        value["producers"][1]["workflow_job_key"] = serde_json::json!("task-linux");
    });
    assert_sidecar_rejected("unsuccessful producer", |value| {
        value["producers"][1]["conclusion"] = serde_json::json!("skipped");
    });
    assert_sidecar_rejected("nonpositive artifact id", |value| {
        value["producers"][0]["artifact_id"] = serde_json::json!(0);
    });
    assert_sidecar_rejected("nonpositive check run id", |value| {
        value["producers"][0]["check_run_id"] = serde_json::json!(-1);
    });
    assert_sidecar_rejected("unsupported origin", |value| {
        value["origin"] = serde_json::json!("github_enterprise");
    });
    assert_sidecar_rejected("unknown field", |value| {
        value["untrusted"] = serde_json::json!(true);
    });
}

fn assert_sidecar_rejected(case: &str, mutate: impl FnOnce(&mut serde_json::Value)) {
    let mut value = task_report_outputs();
    mutate(&mut value);
    assert!(
        serde_json::from_value::<TaskReportOutputFanIn>(value).is_err(),
        "accepted invalid case: {case}"
    );
}

#[test]
fn merge_request_rejects_unknown_fields() {
    let err = serde_json::from_value::<MergeRequest>(serde_json::json!({
        "schema": 1,
        "run_key": "run",
        "matrix_reports": [],
        "required_job_ids": [],
        "required_jobs": [],
        "smuggled": true,
    }))
    .expect_err("unknown field must fail");
    assert!(err.to_string().contains("smuggled"), "unexpected: {err}");
}

#[test]
fn baseline_manifest_round_trip() {
    let manifest: BaselineManifest = serde_json::from_value(serde_json::json!({
        "schema": 2,
        "repository_id": "repo",
        "source_commit": "abc",
        "ref": "refs/heads/main",
        "event": "push",
        "workflow_ref": "w",
        "run_id": 7,
        "run_attempt": 1,
        "final_status": "passed",
        "generator_version": "v",
        "generator_sha256": "s",
        "compatibility_id": "c",
        "artifact_id": 9,
        "artifact_name": "n",
        "tasks": [],
    }))
    .expect("manifest");
    assert_eq!(manifest.schema, 2);
    assert!(manifest.expires_at_unix.is_none());
    let back = serde_json::to_value(&manifest).expect("serialize");
    assert_eq!(back["ref"], "refs/heads/main");
}
