//! Retried publication preserves immutable evidence from its original attempt.

use super::*;

#[test]
fn publication_event_payload_rejects_duplicate_keys_before_staging() {
    let head = "a".repeat(40);
    let temp = tempfile::tempdir().expect("temp");
    for repeated in [
        "\"ref\":\"refs/heads/testmain\",\"ref\":\"refs/heads/other\"",
        "\"repository\":{\"default_branch\":\"testmain\",\"default_branch\":\"other\"}",
    ] {
        let payload = format!(
            "{{{repeated},\"before\":\"{}\",\"after\":\"{head}\"}}",
            "b".repeat(40)
        );
        let path = temp.path().join("request.json");
        let result = write_publish_request(
            &path,
            "push",
            &payload,
            Some(&head),
            Some("o/r"),
            temp.path(),
        );
        assert!(
            result
                .expect_err("duplicate keys")
                .to_string()
                .contains("malformed_event_payload")
        );
        assert!(!path.exists(), "malformed payload stages no request");
    }
}

/// Existing service-authenticated artifact from the successful first attempt.
fn existing() -> BaselineManifest {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let request = publish_request(&request_json(&head)).expect("request");
    publish_manifest(&request, &plan, 7, 1, None).expect("original")
}

/// No new baseline bytes may appear when the existing artifact qualifies.
fn assert_not_staged(temp: &Path, run_key: &str) {
    assert!(
        !temp
            .join("velnor")
            .join(run_key)
            .join("published")
            .join(BASELINE_FILENAME)
            .exists()
    );
}

#[test]
fn successful_retry_references_original_attempt_without_upload() {
    let manifest = existing();
    let plan = fixture_plan(&manifest.source_commit, "r7-a2");
    let temp = staged_run(&plan, "r7-a2");
    for _ in 0..2 {
        let outputs = baseline_publish_with_lookup(
            &request_json(&plan.head),
            "r7-a2",
            temp.path(),
            |_, _, run_id, _| {
                assert_eq!(run_id, manifest.run_id);
                Ok(Some(manifest.clone()))
            },
        )
        .expect("idempotent retry");
        assert!(!outputs.upload_needed);
        assert_eq!(outputs.artifact_name, manifest.artifact_name);
        assert_eq!(outputs.baseline_run_attempt, 1);
        assert_not_staged(temp.path(), "r7-a2");
    }
}

#[test]
fn verified_absence_produces_new_current_attempt_manifest() {
    let plan = fixture_plan(&"a".repeat(40), "r7-a2");
    let temp = staged_run(&plan, "r7-a2");
    let outputs = publish_fixture(&request_json(&plan.head), "r7-a2", temp.path()).expect("new");
    assert!(outputs.upload_needed);
    assert_eq!(outputs.baseline_run_attempt, 2);
    assert_eq!(staged_manifest(temp.path(), "r7-a2")["run_attempt"], 2);
}

#[test]
fn existing_baseline_requires_complete_exact_successful_semantics() {
    let original = existing();
    for mutation in [
        "failed",
        "future_attempt",
        "wrong_run",
        "source",
        "generator",
        "missing",
        "extra",
        "input",
        "task",
        "closure",
        "expired",
        "unverified_origin",
    ] {
        let mut manifest = original.clone();
        match mutation {
            "failed" => manifest.final_status = "failed".to_owned(),
            "future_attempt" => manifest.run_attempt = 3,
            "wrong_run" => manifest.run_id = 99,
            "source" => manifest.source_commit = "b".repeat(40),
            "generator" => manifest.generator_sha256 = "b".repeat(64),
            "missing" => {
                manifest.tasks.pop();
            }
            "extra" => manifest.tasks.push(manifest.tasks[0].clone()),
            "input" => manifest.tasks[0].input_digest = digest(9),
            "task" => manifest.tasks[0].task_digest = digest(9),
            "closure" => manifest.tasks[0].closure_digest = digest(9),
            "expired" => manifest.expires_at_unix = Some(1),
            "unverified_origin" => manifest.tasks[0].proof_run_id = 99,
            _ => unreachable!("closed mutations"),
        }
        let plan = fixture_plan(&original.source_commit, "r7-a2");
        let temp = staged_run(&plan, "r7-a2");
        let result = baseline_publish_with_lookup(
            &request_json(&plan.head),
            "r7-a2",
            temp.path(),
            |_, _, _, _| Ok(Some(manifest)),
        );
        assert!(result.is_err(), "{mutation}");
        assert_not_staged(temp.path(), "r7-a2");
    }
}

#[test]
fn unavailable_service_never_counts_as_successful_idempotence() {
    let plan = fixture_plan(&"a".repeat(40), "r7-a2");
    let temp = staged_run(&plan, "r7-a2");
    let result = baseline_publish_with_lookup(
        &request_json(&plan.head),
        "r7-a2",
        temp.path(),
        |_, _, _, _| Err(internal("publish_refused:existing_baseline:unavailable")),
    );
    assert!(
        result
            .expect_err("unavailable")
            .to_string()
            .contains("unavailable")
    );
    assert_not_staged(temp.path(), "r7-a2");
}

#[test]
fn failed_retry_cannot_use_an_existing_successful_artifact() {
    let plan = fixture_plan(&"a".repeat(40), "r7-a2");
    let temp = staged_run(&plan, "r7-a2");
    let path = temp.path().join("velnor/r7-a2/final-report.json");
    let mut report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("read")).expect("report");
    report["status"] = serde_json::json!("failed");
    fs::write(path, report.to_string()).expect("write");
    let result = baseline_publish_with_lookup(
        &request_json(&plan.head),
        "r7-a2",
        temp.path(),
        |_, _, _, _| panic!("failed current report must refuse before artifact lookup"),
    );
    assert!(result.is_err());
    assert_not_staged(temp.path(), "r7-a2");
}
