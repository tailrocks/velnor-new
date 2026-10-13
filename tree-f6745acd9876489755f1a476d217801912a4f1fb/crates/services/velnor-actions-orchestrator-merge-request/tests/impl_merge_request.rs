use velnor_actions_contract::{digest_b3, task_report_id_for_task};
use velnor_actions_orchestrator_merge_request::{
    MAX_STAGED_REPORT_BYTES, assemble_with_needs, write_merge_request_to,
};

const AID: &str = "velnor-crate-local-crate_demo";
const KEY: &str = "m-0123456789abcdef";

/// Run directory with caller-supplied plan plus caller-supplied files.
fn staged(plan: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("plan.json"), plan).expect("plan");
    std::fs::write(dir.path().join("matrix.json"), "{}").expect("matrix");
    for (name, body) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parents");
        }
        std::fs::write(&path, body).expect("file");
    }
    dir
}

/// Minimal plan JSON naming `(artifact-id, matrix-key)` entries.
fn plan_with(entries: &[(&str, &str)]) -> String {
    let include: Vec<String> = entries
        .iter()
        .map(|(id, key)| {
            format!(
                r#"{{"artifact_id":"{id}","matrix_key":"{key}","report_id":"report-for-{id}-{key}"}}"#
            )
        })
        .collect();
    format!(r#"{{"matrix":{{"include":[{}]}}}}"#, include.join(","))
}

/// Error strings of one assembled request.
fn error_list(request: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(request).expect("json")["assembly_errors"]
        .as_array()
        .expect("errors")
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

fn assemble(run_dir: &std::path::Path) -> String {
    assemble_with_needs(
        "local",
        run_dir,
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("assemble")
}

#[test]
fn request_shape_carries_schema_run_key_and_sections() {
    let dir = staged(&plan_with(&[]), &[]);
    let value: serde_json::Value = serde_json::from_str(&assemble(dir.path())).expect("json");
    assert_eq!(value["schema"], 1);
    assert_eq!(value["run_key"], "local");
    for section in [
        "plan",
        "matrix",
        "matrix_reports",
        "task_reports",
        "check_proofs",
        "required_job_ids",
        "required_jobs",
        "assembly_errors",
        "baseline_manifest",
    ] {
        assert!(value.get(section).is_some(), "missing {section}");
    }
}

#[test]
fn missing_inputs_are_recorded_never_dropped() {
    let empty = tempfile::TempDir::new().expect("tempdir");
    let errors = error_list(&assemble(empty.path()));
    for want in ["missing_plan", "missing_matrix"] {
        assert!(errors.contains(&want.to_owned()), "{errors:?}");
    }
}

#[test]
fn staged_reports_land_sorted_in_matrix_reports() {
    let dir = staged(
        &plan_with(&[(AID, "m-0000000000000002"), (AID, "m-0000000000000001")]),
        &[
            (
                "reports/velnor-crate-local-crate_demo/m-0000000000000002/matrix-report.json",
                r#"{"report_id":"report-2"}"#,
            ),
            (
                "reports/velnor-crate-local-crate_demo/m-0000000000000001/matrix-report.json",
                r#"{"report_id":"report-1"}"#,
            ),
        ],
    );
    let value: serde_json::Value = serde_json::from_str(&assemble(dir.path())).expect("json");
    let ids: Vec<&str> = value["matrix_reports"]
        .as_array()
        .expect("reports")
        .iter()
        .map(|report| report["report_id"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(ids, ["report-1", "report-2"]);
}

#[test]
fn stray_files_are_ignored() {
    let dir = staged(
        &plan_with(&[]),
        &[("reports/stray.json", r#"{"report_id":"stray"}"#)],
    );
    let value: serde_json::Value = serde_json::from_str(&assemble(dir.path())).expect("json");
    assert_eq!(value["matrix_reports"].as_array().map(Vec::len), Some(0));
    assert!(error_list(&serde_json::to_string(&value).expect("json")).is_empty());
}

#[test]
fn malformed_artifact_id_is_recorded() {
    let dir = staged(&plan_with(&[("not an id", KEY)]), &[]);
    let errors = error_list(&assemble(dir.path()));
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("bad_artifact_id")),
        "{errors:?}"
    );
}

#[test]
fn unparsable_report_is_recorded() {
    let dir = staged(
        &plan_with(&[(AID, KEY)]),
        &[(
            "reports/velnor-crate-local-crate_demo/m-0123456789abcdef/matrix-report.json",
            "not json",
        )],
    );
    let errors = error_list(&assemble(dir.path()));
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("unparsable_report:")),
        "{errors:?}"
    );
}

#[test]
fn task_files_land_in_task_reports() {
    let task_id = "stack/rust/root/clippy/default";
    let digest = digest_b3(b"task");
    let plan = serde_json::json!({
        "matrix": {"include": [{
            "artifact_id": AID,
            "matrix_key": KEY,
            "execute_task_ids": {"tasks": {"clippy": task_id}},
        }]},
        "obligations": [{"task_id": task_id, "task_digest": digest}],
    });
    let file_id = task_report_id_for_task("local", KEY, &digest).expect("task file id");
    let dir = staged(
        &plan.to_string(),
        &[
            (
                "reports/velnor-crate-local-crate_demo/m-0123456789abcdef/matrix-report.json",
                r#"{"report_id":"r"}"#,
            ),
            (
                &format!(
                    "reports/velnor-crate-local-crate_demo/m-0123456789abcdef/tasks/{file_id}.json"
                ),
                r#"{"task_report_id":"t"}"#,
            ),
        ],
    );
    let value: serde_json::Value = serde_json::from_str(&assemble(dir.path())).expect("json");
    assert_eq!(value["task_reports"].as_array().map(Vec::len), Some(1));
}

#[test]
fn missing_task_file_is_recorded() {
    let task_id = "stack/rust/root/clippy/default";
    let digest = digest_b3(b"task");
    let plan = serde_json::json!({
        "matrix": {"include": [{
            "artifact_id": AID,
            "matrix_key": KEY,
            "execute_task_ids": {"tasks": {"clippy": task_id}},
        }]},
        "obligations": [{"task_id": task_id, "task_digest": digest}],
    });
    let dir = staged(
        &plan.to_string(),
        &[(
            "reports/velnor-crate-local-crate_demo/m-0123456789abcdef/matrix-report.json",
            r#"{"report_id":"r"}"#,
        )],
    );
    let errors = error_list(&assemble(dir.path()));
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("missing_task:")),
        "{errors:?}"
    );
}

#[test]
fn oversize_report_is_recorded() {
    let big = "x".repeat(usize::try_from(MAX_STAGED_REPORT_BYTES + 10).expect("bound"));
    let dir = staged(
        &plan_with(&[(AID, KEY)]),
        &[(
            "reports/velnor-crate-local-crate_demo/m-0123456789abcdef/matrix-report.json",
            &big,
        )],
    );
    let errors = error_list(&assemble(dir.path()));
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("oversize_report:")),
        "{errors:?}"
    );
}

#[test]
fn canonical_encoding_is_deterministic() {
    let dir = staged(
        &plan_with(&[(AID, KEY)]),
        &[(
            "reports/velnor-crate-local-crate_demo/m-0123456789abcdef/matrix-report.json",
            r#"{"report_id":"r"}"#,
        )],
    );
    let first = assemble(dir.path());
    let second = assemble(dir.path());
    assert_eq!(first, second);
}

#[test]
fn write_to_creates_request_file_exclusively() {
    let anchor = tempfile::TempDir::new().expect("anchor");
    let run_dir = anchor.path().join("velnor").join("local");
    std::fs::create_dir_all(&run_dir).expect("run dir");
    std::fs::write(run_dir.join("plan.json"), "{}").expect("plan");
    std::fs::write(run_dir.join("matrix.json"), "{}").expect("matrix");
    let request_path = anchor.path().join("merge-request.json");
    let written =
        write_merge_request_to(&request_path, "local", &run_dir, anchor.path()).expect("write");
    assert_eq!(written, request_path);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&request_path).expect("read")).expect("json");
    assert_eq!(value["schema"], 1);
    assert_eq!(value["run_key"], "local");
    write_merge_request_to(&request_path, "local", &run_dir, anchor.path()).expect_err("exclusive");
}

#[test]
fn write_to_refuses_anchor_escape() {
    let anchor = tempfile::TempDir::new().expect("anchor");
    let outside = tempfile::TempDir::new().expect("outside");
    let request_path = outside.path().join("merge-request.json");
    write_merge_request_to(&request_path, "local", anchor.path(), anchor.path())
        .expect_err("escape");
}
