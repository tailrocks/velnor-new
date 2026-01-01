//! Assembly tests.
//!
//! Declared via `#[path]` from `merge_request.rs` under `cfg(test)`.

use super::*;
use crate::merge::required_evidence::MAX_BASELINE_MANIFEST_BYTES;

/// Minimal plan JSON naming `(artifact-id, matrix-key)` entries.
pub(crate) fn plan_with(entries: &[(&str, &str)]) -> String {
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

/// Run directory with caller-supplied plan plus caller-supplied files.
pub(crate) fn staged(plan: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
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

/// Error strings of one assembled request.
pub(crate) fn error_list(request: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(request).expect("json")["assembly_errors"]
        .as_array()
        .expect("errors")
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

#[test]
fn assembly_shape_carries_no_base() {
    let aid = "velnor-crate-local-crate_demo";
    let dir = staged(
        &plan_with(&[(aid, "m-0123456789abcdef")]),
        &[(
            "reports/velnor-crate-local-crate_demo/m-0123456789abcdef/matrix-report.json",
            r#"{"report_id":"b"}"#,
        )],
    );
    let needs = r#"{"plan":"success","rust-demo":"success"}"#;
    let expected = r#"["plan","rust-demo"]"#;
    let request = assemble_with_needs(
        "local",
        dir.path(),
        Some(needs),
        Some(expected),
        Some("push"),
        Some("{}"),
    )
    .expect("assemble");
    let value: serde_json::Value = serde_json::from_str(&request).expect("json");
    assert!(value.get("base").is_none(), "{request}");
    assert_eq!(value["schema"], 1);
    assert_eq!(value["matrix_reports"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        value["required_job_ids"],
        serde_json::json!(["plan", "rust-demo"])
    );
    assert!(error_list(&request).is_empty(), "{request}");
}

#[test]
fn assembly_reads_expected_only_and_sorts_reports() {
    let shared = "velnor-crate-local-crate_demo";
    let dir = staged(
        &plan_with(&[
            (shared, "m-0000000000000001"),
            (shared, "m-0000000000000002"),
        ]),
        &[
            (
                "reports/velnor-crate-local-crate_demo/m-0000000000000002/matrix-report.json",
                r#"{"report_id":"report-2"}"#,
            ),
            (
                "reports/velnor-crate-local-crate_demo/m-0000000000000001/matrix-report.json",
                r#"{"report_id":"report-1"}"#,
            ),
            ("reports/stray.json", r#"{"report_id":"stray"}"#),
        ],
    );
    let needs = r#"{"plan":{"result":"success","outputs":{}}}"#;
    let request = assemble_with_needs(
        "local",
        dir.path(),
        Some(needs),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("assemble");
    let value: serde_json::Value = serde_json::from_str(&request).expect("json");
    let ids: Vec<&str> = value["matrix_reports"]
        .as_array()
        .expect("reports")
        .iter()
        .map(|report| report["report_id"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(ids, ["report-1", "report-2"]);
    assert!(error_list(&request).is_empty(), "{request}");
}

#[test]
fn assembly_records_gaps_and_rejects_bad_report() {
    let empty = tempfile::TempDir::new().expect("tempdir");
    let request = assemble_with_needs("local", empty.path(), None, None, Some("push"), Some("{}"))
        .expect("null plan");
    let value: serde_json::Value = serde_json::from_str(&request).expect("json");
    assert!(value["plan"].is_null(), "{request}");
    assert!(value["matrix"].is_null(), "{request}");
    assert_eq!(value["required_job_ids"].as_array().map(Vec::len), Some(0));
    let errors = error_list(&request);
    for want in ["missing_plan", "missing_matrix", "missing_needs_channel"] {
        assert!(errors.contains(&want.to_owned()), "{errors:?}");
    }
    let aid = "velnor-crate-local-crate_demo";
    let bad = staged(
        &plan_with(&[(aid, "m-0123456789abcdef"), (aid, "bogus")]),
        &[(
            "reports/velnor-crate-local-crate_demo/m-0123456789abcdef/matrix-report.json",
            "not json",
        )],
    );
    let request = assemble_with_needs(
        "local",
        bad.path(),
        Some(r#"{"a":"b"}"#),
        Some(r#"["a"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("diagnostic");
    let errors = error_list(&request);
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("unparsable_report:")),
        "{errors:?}"
    );
    assert!(
        errors.contains(&format!("bad_matrix_key:{aid}")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("bad_needs_result:")),
        "{errors:?}"
    );
    let missing = staged(&plan_with(&[(aid, "m-0123456789abcdef")]), &[]);
    let request = assemble_with_needs(
        "local",
        missing.path(),
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("diagnostic");
    let errors = error_list(&request);
    assert!(
        errors
            .iter()
            .any(|error| error.starts_with("missing_report:")),
        "{errors:?}"
    );
}

#[test]
fn assembly_rejects_links_oversize_and_unreadable_inputs() {
    let dir = staged("{}", &[]);
    std::fs::write(dir.path().join("real.json"), "{}").expect("real");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        dir.path().join("real.json"),
        dir.path().join("baseline.json"),
    )
    .expect("link");
    #[cfg(unix)]
    {
        let request = assemble_with_needs(
            "local",
            dir.path(),
            Some(r#"{"plan":"success"}"#),
            Some(r#"["plan"]"#),
            Some("push"),
            Some("{}"),
        )
        .expect("asm");
        let errors = error_list(&request);
        assert!(
            errors.contains(&"symlink_baseline".to_owned()),
            "symlinked baseline.json reports: {errors:?}"
        );
        std::fs::remove_file(dir.path().join("baseline.json")).expect("rm");
    }
    std::fs::write(
        dir.path().join("plan.json"),
        "x".repeat(usize::try_from(MAX_ASSEMBLY_JSON_BYTES + 10).expect("bound")),
    )
    .expect("big");
    let request = assemble_with_needs(
        "local",
        dir.path(),
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("asm");
    let errors = error_list(&request);
    assert!(errors.contains(&"oversize_plan".to_owned()), "{errors:?}");
    std::fs::write(dir.path().join("plan.json"), "{}").expect("restore plan");
    std::fs::create_dir(dir.path().join("baseline.json")).expect("dir");
    let request = assemble_with_needs(
        "local",
        dir.path(),
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("asm");
    let errors = error_list(&request);
    assert!(
        errors.contains(&"unreadable_baseline".to_owned()),
        "{errors:?}"
    );
}

#[test]
fn baseline_raw_limit_covers_nested_parents_and_padding() {
    let dir = staged("{}", &[]);
    let nested = serde_json::json!({"parent": {"parent": {"parent": null}}}).to_string();
    let padded_len = MAX_BASELINE_MANIFEST_BYTES + 1;
    let padded = format!("{nested}{}", " ".repeat(padded_len - nested.len()));
    std::fs::write(dir.path().join("baseline.json"), padded).expect("padded baseline");
    let request = assemble_with_needs(
        "local",
        dir.path(),
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("assemble");
    assert!(
        error_list(&request).contains(&"oversize_baseline".to_owned()),
        "{request}"
    );

    let expanded = serde_json::json!({
        "parent": {"parent": null},
        "padding": "x".repeat(padded_len),
    })
    .to_string();
    std::fs::write(dir.path().join("baseline.json"), expanded).expect("expanded baseline");
    let request = assemble_with_needs(
        "local",
        dir.path(),
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("assemble");
    assert!(
        error_list(&request).contains(&"oversize_baseline".to_owned()),
        "{request}"
    );
}

#[test]
fn dropped_or_missing_expected_inventory_fails_closed() {
    let dir = staged(&plan_with(&[]), &[]);
    let dropped = assemble_with_needs(
        "local",
        dir.path(),
        Some(r#"{"plan":"success"}"#),
        Some(r#"["lint","plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("diagnostic");
    let errors = error_list(&dropped);
    assert!(
        errors.contains(&"needs_inventory_mismatch".to_owned()),
        "{errors:?}"
    );
    let value: serde_json::Value = serde_json::from_str(&dropped).expect("json");
    assert_eq!(
        value["required_job_ids"],
        serde_json::json!(["lint", "plan"]),
        "inventory binds to expected, not observed"
    );
    let missing = assemble_with_needs(
        "local",
        dir.path(),
        Some(r#"{"plan":"success"}"#),
        None,
        Some("push"),
        Some("{}"),
    )
    .expect("diagnostic");
    let errors = error_list(&missing);
    assert!(
        errors.contains(&"missing_needs_expected".to_owned()),
        "{errors:?}"
    );
}

#[test]
fn assembly_rejects_duplicate_keys_in_staged_json() {
    let aid = "velnor-crate-local-crate_demo";
    let key = "m-0123456789abcdef";
    // A duplicated critical key in plan.json collapses under lenient
    // parsing; strict assembly records it instead of judging the
    // winner.
    let dir = staged(r#"{"matrix":{"include":[]},"matrix":{"include":[]}}"#, &[]);
    let request = assemble_with_needs(
        "local",
        dir.path(),
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("diagnostic");
    assert!(
        error_list(&request).contains(&"unparsable_plan".to_owned()),
        "dup-key plan must fail assembly: {request}"
    );
    // Same for a staged matrix report: last-wins is never evidence.
    let dir = staged(
        &plan_with(&[(aid, key)]),
        &[(
            "reports/velnor-crate-local-crate_demo/m-0123456789abcdef/matrix-report.json",
            r#"{"report_id":"a","report_id":"b"}"#,
        )],
    );
    let request = assemble_with_needs(
        "local",
        dir.path(),
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("push"),
        Some("{}"),
    )
    .expect("diagnostic");
    assert!(
        error_list(&request).contains(&format!("unparsable_report:{aid}")),
        "dup-key report must fail assembly: {request}"
    );
}

#[test]
fn request_file_writes_exclusively() {
    let dir = staged("{}", &[]);
    let file = dir.path().join("sub").join("merge-v1-request.json");
    let written = write_merge_request_to(&file, "local", dir.path(), dir.path()).expect("write");
    assert_eq!(written, file);
    let err = write_merge_request_to(&file, "local", dir.path(), dir.path()).expect_err("exists");
    assert!(err.to_string().contains("request_exists"), "{err}");
}

/// Symlinked event payloads read as absent: assembly records the gap
/// instead of following the link.
#[test]
fn event_payload_refuses_symlink() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let real = dir.path().join("event.json");
    std::fs::write(&real, "{}").expect("payload");
    let via = dir.path().join("linked.json");
    std::os::unix::fs::symlink(&real, &via).expect("link");
    assert_eq!(event_payload_from(real.as_os_str()), Some("{}".to_owned()));
    assert_eq!(event_payload_from(via.as_os_str()), None);
}
