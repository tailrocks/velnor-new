use std::fs;

use super::{fixture_data, prepare_api_fixture, run_api_script};

#[test]
fn actual_stock_miss_candidates_require_verified_logs_and_keep_api_identity() {
    let root = fixture_data::TestDirectory::new();
    let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, true);
    let output = run_api_script(&root.0, &runner_temp, &jobs_path);
    assert!(
        output.status.success(),
        "verified stock fixture failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence = runner_temp.join("mbx-cache-evidence");
    let receipt = evidence.join("parallel-api-receipt.json");
    assert_eq!(fixture_data::jq_value(".classification", &receipt), "RUN");
    assert_eq!(
        fixture_data::jq_value(".seed.restore_classification", &receipt),
        "CLEAN_MISS"
    );
    assert_eq!(
        fixture_data::jq_value(
            ".parallel_intervals | map(.restore_classification) | join(\",\")",
            &receipt,
        ),
        "HIT,HIT,CLEAN_MISS"
    );
    assert_eq!(
        fixture_data::jq_value(".observer_shared_restore_classification", &receipt),
        "HIT"
    );
    for (role, classification, job_id) in [
        ("seed", "CLEAN_MISS", "11"),
        ("reader-a", "HIT", "12"),
        ("reader-b", "HIT", "13"),
        ("new-key-writer", "CLEAN_MISS", "14"),
        ("observer-shared", "HIT", "15"),
    ] {
        let sidecar = evidence.join(format!("stock-restore-{role}.json"));
        assert_eq!(
            fixture_data::jq_value(".classification", &sidecar),
            classification
        );
        assert_eq!(fixture_data::jq_value(".api_job_id", &sidecar), job_id);
        assert_eq!(fixture_data::jq_value(".restore_step_index", &sidecar), "0");
        assert_eq!(fixture_data::jq_value(".workflow_id", &sidecar), "456");
    }
}

#[test]
fn documented_and_explicit_default_branch_workflow_paths_are_accepted() {
    for suffix in ["@main", "@refs/heads/main"] {
        let root = fixture_data::TestDirectory::new();
        let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, true);
        let run_path = root.0.join("stock-run.json");
        let run = fs::read_to_string(&run_path).expect("read workflow run fixture");
        fs::write(
            run_path,
            run.replace(
                ".github/workflows/qualification.yml@main",
                &format!(".github/workflows/qualification.yml{suffix}"),
            ),
        )
        .expect("write workflow path fixture");
        let output = run_api_script(&root.0, &runner_temp, &jobs_path);
        assert!(
            output.status.success(),
            "workflow path {suffix} failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn cold_candidates_fail_closed_on_nonclean_or_unmatched_step_logs() {
    for (role, filename, log) in [
        (
            "seed",
            "seed.log",
            "2026-10-04T00:00:00.0000000Z FailedToRestore: corrupt archive\n",
        ),
        (
            "seed warning",
            "seed.log",
            "2026-10-04T00:00:00.0000000Z warning: cache miss\n",
        ),
        (
            "writer error",
            "writer.log",
            "2026-10-04T00:00:00.0000000Z error: cache miss\n",
        ),
        (
            "seed wrong key",
            "seed.log",
            "2026-10-04T00:00:00.0000000Z Cache not found for input keys: wrong-key\n",
        ),
        (
            "writer duplicate miss",
            "writer.log",
            concat!(
                "2026-10-04T00:00:00.0000000Z Cache not found for input keys: ",
                "linux-x64-mbx-velnor-mbx-1.22.0-dir-rust-1.98.1-",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-",
                "scope-f4e55cb8bd2ee005472379b3c915d9de2973e186fbfbc5db6aee443635c5a135-",
                "run-123-attempt-2-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
                "2026-10-04T00:00:01.0000000Z Cache not found for input keys: ",
                "linux-x64-mbx-velnor-mbx-1.22.0-dir-rust-1.98.1-",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-",
                "scope-f4e55cb8bd2ee005472379b3c915d9de2973e186fbfbc5db6aee443635c5a135-",
                "run-123-attempt-2-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
            ),
        ),
    ] {
        let root = fixture_data::TestDirectory::new();
        let (runner_temp, jobs_path) = prepare_api_fixture(&root.0, true);
        fs::write(root.0.join(filename), log).expect("replace fixture restore log");
        let output = run_api_script(&root.0, &runner_temp, &jobs_path);
        assert!(
            !output.status.success(),
            "parallel observer accepted nonclean {role} log"
        );
    }
}
