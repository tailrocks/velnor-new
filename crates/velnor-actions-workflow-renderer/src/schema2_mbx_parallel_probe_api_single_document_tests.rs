use std::fs;

use super::{
    fixture_data, prepare_api_fixture, run_api_script,
    run_api_script_for_platform_with_duplicate_jobs,
};

#[test]
fn duplicate_top_level_json_documents_fail_closed_as_not_run() {
    for (duplicate, relative_path, duplicate_jobs) in [
        ("jobs response", None, true),
        (
            "seed receipt",
            Some("mbx-parallel-input/seed/cache-receipt.json"),
            false,
        ),
        ("workflow event", Some("event.json"), false),
    ] {
        let root = fixture_data::TestDirectory::new();
        let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, true);
        if let Some(relative_path) = relative_path {
            append_duplicate_document(&runner_temp.join(relative_path));
        }
        let output = if duplicate_jobs {
            run_api_script_for_platform_with_duplicate_jobs(
                &root.0,
                &runner_temp,
                &jobs_path,
                "Linux",
                "X64",
                "x64",
                true,
            )
        } else {
            run_api_script(&root.0, &runner_temp, &jobs_path)
        };
        assert!(
            !output.status.success(),
            "accepted duplicate top-level JSON in {duplicate}"
        );
        assert_eq!(
            fs::read_to_string(
                runner_temp.join("mbx-cache-evidence/seed-restore-classification.txt")
            )
            .expect("read seed classifier result"),
            "NOT_RUN\n",
            "duplicate top-level JSON in {duplicate} was not classified NOT_RUN"
        );
    }
}

fn append_duplicate_document(path: &std::path::Path) {
    let original = fs::read_to_string(path).expect("read fixture JSON document");
    fs::write(path, format!("{original}\n{original}\n"))
        .expect("duplicate fixture JSON document");
}
