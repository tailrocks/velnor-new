use velnor_actions_orchestrator_merge_ports::{
    BaselineManifest, MergeRequest, SCHEMA, Signals, TestIdentity, check_schema, inventory_digest,
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
