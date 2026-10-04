use std::fs;

use super::{fixture_data, prepare_api_fixture, run_api_script};

#[test]
fn second_attempt_jobs_response_document_fails_before_parallel_receipt() {
    let root = fixture_data::TestDirectory::new();
    let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, true);
    let valid = fs::read_to_string(&jobs_path).expect("read current-attempt jobs response");
    fs::write(
        &jobs_path,
        format!("{valid}\n{{\"jobs\":[],\"total_count\":0}}\n"),
    )
    .expect("append invalid second jobs document");

    let output = run_api_script(&root.0, &runner_temp, &jobs_path);

    assert!(
        !output.status.success(),
        "parallel API accepted an extra current-attempt jobs document: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !runner_temp
            .join("mbx-cache-evidence/parallel-api-receipt.json")
            .exists(),
        "parallel API wrote its final receipt before rejecting the extra document"
    );
}
