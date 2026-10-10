//! Event-time protocol: request materialization, outputs, verdict.

use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::canonical_json_str;
use velnor_actions_orchestrator::{
    PlanOutputMode, plan_internal, plan_outputs, response_path_for, write_request_parts,
};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, plan_for_source_change,
};
use crate::impl_orch_core_cover::covered_plan;

#[path = "impl_protocol_merge.rs"]
mod merge_tests;

#[test]
fn write_request_materializes_pull_request() -> TestResult {
    let dir = TempDir::new()?;
    let base = "b".repeat(64);
    let head = "a".repeat(64);
    let payload = format!(
        "{{\"pull_request\":{{\"base\":{{\"sha\":\"{base}\"}},\"head\":{{\"sha\":\"{head}\",\"repo\":{{\"fork\":false}}}}}}}}"
    );
    let file = dir
        .path()
        .join("velnor")
        .join("request")
        .join("plan-v1-request.json");
    let written = write_request_parts(&file, "pull_request", &payload, None, None, dir.path())?;
    assert_eq!(written, file);
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["schema"], 1);
    assert_eq!(value["op"], "plan-v1");
    assert_eq!(value["event"], "pull_request");
    assert_eq!(value["base"], base);
    assert_eq!(value["head"], head);
    assert_eq!(value["root"], ".");
    let keys: Vec<&str> = value
        .as_object()
        .map(|map| map.keys().map(String::as_str).collect())
        .unwrap_or_default();
    assert_eq!(keys, ["base", "event", "head", "op", "root", "schema"]);
    Ok(())
}

#[test]
fn write_request_captures_repository_capability() -> TestResult {
    let dir = TempDir::new()?;
    let head = "a".repeat(40);
    let payload = format!("{{\"before\":null,\"after\":\"{head}\"}}");
    let file = dir.path().join("plan-v1-request.json");
    write_request_parts(&file, "push", &payload, None, Some("o/r"), dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["repository"], "o/r");
    let bare = dir.path().join("bare").join("plan-v1-request.json");
    write_request_parts(&bare, "push", &payload, None, None, dir.path())?;
    let bare_value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&bare)?)?;
    assert!(
        bare_value.get("repository").is_none(),
        "unset slug stays omitted: {bare_value}"
    );
    Ok(())
}

#[test]
fn write_request_materializes_push_without_base() -> TestResult {
    let dir = TempDir::new()?;
    let head = "c".repeat(40);
    let payload = format!("{{\"before\":\"{}\",\"after\":\"{head}\"}}", "0".repeat(40));
    let file = dir.path().join("plan-v1-request.json");
    write_request_parts(&file, "push", &payload, Some(&head), None, dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["event"], "push");
    assert!(value["base"].is_null(), "zero before maps to null");
    assert_eq!(value["head"], head);

    let sha = "d".repeat(40);
    let fallback = dir.path().join("sub").join("plan-v1-request.json");
    let nulls = format!(
        "{{\"before\":\"{}\",\"after\":\"{}\"}}",
        "0".repeat(40),
        "0".repeat(40)
    );
    write_request_parts(&fallback, "push", &nulls, Some(&sha), None, dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&fallback)?)?;
    assert_eq!(value["head"], sha);
    Ok(())
}

#[test]
fn write_request_materializes_merge_group() -> TestResult {
    let dir = TempDir::new()?;
    let base = "d".repeat(40);
    let head = "e".repeat(40);
    let payload =
        format!("{{\"merge_group\":{{\"base_sha\":\"{base}\",\"head_sha\":\"{head}\"}}}}");
    let file = dir.path().join("plan-v1-request.json");
    write_request_parts(&file, "merge_group", &payload, None, None, dir.path())?;
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)?;
    assert_eq!(value["op"], "plan-v1");
    assert_eq!(value["event"], "merge_group");
    assert_eq!(value["base"], base);
    assert_eq!(value["head"], head);
    Ok(())
}

#[test]
fn write_request_rejects_bad_inputs() -> TestResult {
    let dir = TempDir::new()?;
    let payload = r#"{"before":"abc","after":"def"}"#;
    let bad_op = dir.path().join("bogus-v9-request.json");
    let err = err_of(
        write_request_parts(&bad_op, "push", payload, None, None, dir.path()),
        "unknown op refused",
    )?;
    assert!(err.to_string().contains("unknown_request_op"), "{err}");
    assert!(!bad_op.exists());
    let bad_event = dir.path().join("plan-v1-request.json");
    let err = err_of(
        write_request_parts(&bad_event, "schedule", payload, None, None, dir.path()),
        "unknown event refused",
    )?;
    assert!(err.to_string().contains("unsupported_event"), "{err}");
    let err = err_of(
        write_request_parts(&bad_event, "push", "not json", None, None, dir.path()),
        "malformed payload refused",
    )?;
    assert!(err.to_string().contains("malformed_event_payload"), "{err}");

    fs::write(&bad_event, "{}")?;
    let err = err_of(
        write_request_parts(&bad_event, "push", payload, None, None, dir.path()),
        "existing file refused",
    )?;
    assert!(err.to_string().contains("request_exists"), "{err}");
    assert_eq!(fs::read_to_string(&bad_event)?, "{}");
    Ok(())
}

#[test]
fn write_request_pins_parents_to_the_anchor() -> TestResult {
    let dir = TempDir::new()?;
    let payload = r#"{"before":"abc","after":"def"}"#;
    let elsewhere = TempDir::new()?;
    let escaped = elsewhere.path().join("plan-v1-request.json");
    let err = err_of(
        write_request_parts(&escaped, "push", payload, None, None, dir.path()),
        "anchor escape refused",
    )?;
    assert!(err.to_string().contains("anchor_escape"), "{err}");
    assert!(!escaped.exists());
    #[cfg(unix)]
    {
        let real = dir.path().join("real");
        fs::create_dir(&real)?;
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link)?;
        let planted = link.join("plan-v1-request.json");
        let err = err_of(
            write_request_parts(&planted, "push", payload, None, None, dir.path()),
            "linked parent refused",
        )?;
        assert!(err.to_string().contains("symlink_refused"), "{err}");
        assert!(!real.join("plan-v1-request.json").exists());
    }
    Ok(())
}

#[test]
fn response_sibling_derivation() -> TestResult {
    let dir = TempDir::new()?;
    for op in ["plan-v1", "merge-v1"] {
        let request = dir.path().join(format!("{op}-request.json"));
        let sibling = response_path_for(&request)?;
        assert_eq!(sibling, dir.path().join(format!("{op}-response.json")));
    }
    assert!(response_path_for(&dir.path().join("plan-v1.json")).is_err());
    assert!(response_path_for(&dir.path().join("-request.json")).is_err());
    Ok(())
}

#[test]
fn plan_outputs_agree_with_plan_matrix() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let outputs = plan_outputs(&response, PlanOutputMode::Static)?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    assert_eq!(outputs.matrix, canonical_json_str(&value["matrix"])?);
    assert!(!outputs.matrix.contains('\n'));
    assert!(
        outputs.covered_tasks.is_empty(),
        "execute-all plans emit no channel"
    );
    assert!(
        err_of(
            plan_outputs("not json", PlanOutputMode::Static),
            "outputs reject garbage"
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn plan_outputs_encode_covered_tasks() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.task_ids.is_empty(), "fixture must select work");
    let (plan_json, _) = covered_plan(&plan)?;
    let response = serde_json::json!({
        "schema": 1,
        "plan": plan_json,
        "matrix": plan_json["matrix"],
    });
    let outputs = plan_outputs(&response.to_string(), PlanOutputMode::Static)?;
    let mut ids: Vec<&str> = plan
        .obligations
        .iter()
        .map(|obligation| obligation.task_id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(outputs.covered_tasks, format!(",{},", ids.join(",")));
    Ok(())
}

#[test]
fn plan_outputs_bind_qualification_phase_and_cache_policy() -> TestResult {
    let (repo, _) = plan_for_source_change()?;
    let root = repo.path();
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let context = serde_json::json!({
        "campaign": "protocol-test",
        "phase": "cold",
        "repository": "owner/project",
        "default_branch": "testmain",
        "git_ref": "refs/heads/testmain",
        "ref_protected": true,
        "workflow_ref": "owner/project/.github/workflows/ci.yml@refs/heads/testmain",
        "workflow_sha": head,
        "source_sha": head,
        "run_id": 7,
        "run_attempt": 2,
    });
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "r7-a2",
        "base": null,
        "head": head,
        "event": "qualification",
        "qualification": context,
        "root": root.display().to_string(),
        "repository": "owner/project",
    });
    let response = plan_internal(&request.to_string())?;
    let outputs = plan_outputs(&response, PlanOutputMode::Static)?;
    assert_eq!(outputs.qualification_campaign, "protocol-test");
    assert_eq!(outputs.qualification_phase, "cold");
    assert!(outputs.qualification_cache_enabled);
    assert!(!outputs.qualification_cache_write);
    let step_names: Vec<&str> = outputs
        .step_outputs()
        .iter()
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        step_names,
        [
            "matrix",
            "plan_id",
            "run_key",
            "covered_tasks",
            "qualification_campaign",
            "qualification_phase",
            "qualification_cache_enabled",
            "qualification_cache_write",
            "qualification_cache_directives",
        ]
    );
    let directives: serde_json::Value =
        serde_json::from_str(&outputs.qualification_cache_directives)?;
    assert_eq!(directives["schema"], 2);
    assert_eq!(directives["phase"], "cold");
    let lanes = directives["lanes"]
        .as_array()
        .ok_or("qualification cache lanes are not an array")?;
    assert_ne!(lanes.as_slice(), []);
    assert!(lanes.iter().all(|lane| {
        lane["layers"]
            .as_array()
            .is_some_and(|layers| layers.len() == 5)
    }));
    assert!(
        !outputs
            .qualification_cache_directives
            .contains("mbx_bundle")
    );
    let promoted = outputs.promoted_job_outputs(PlanOutputMode::Static);
    let expected_bytes = promoted
        .iter()
        .map(|(name, value)| (name.encode_utf16().count() + value.encode_utf16().count() + 2) * 2)
        .sum::<usize>();
    assert_eq!(outputs.job_outputs_utf16_bytes, expected_bytes);
    Ok(())
}
