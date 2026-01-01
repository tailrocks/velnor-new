use super::*;

/// Error strings of one assembled request.
pub(crate) fn error_list(request: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(request).expect("json")["assembly_errors"]
        .as_array()
        .expect("errors")
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

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

mod actual_event_strict_tests;
mod merge_event_tests;
mod merge_request_tests;
mod task_report_outputs;
