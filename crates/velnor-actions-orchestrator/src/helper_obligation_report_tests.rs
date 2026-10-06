//! A newer plan never authorizes a stale generated native execution frame.

use tempfile::TempDir;

use super::*;

/// Stage a valid neutral plan without pretending its owner is source-qualified.
fn staged_plan() -> (TempDir, Plan) {
    let plan = crate::task_report::task_report_tests::fixture_plan();
    let temp = TempDir::new().expect("temp");
    let run = temp.path().join("velnor").join(&plan.run_key);
    std::fs::create_dir_all(&run).expect("run dir");
    std::fs::write(
        run.join("plan.json"),
        canonical_json_bytes(&plan).expect("plan bytes"),
    )
    .expect("staged plan");
    (temp, plan)
}

#[test]
fn stale_generated_frame_rejected_before_begin_or_terminal_evidence() {
    let (temp, plan) = staged_plan();
    let entry = &plan.matrix.include[0];
    let helper = format!("velnor-helper-{}", entry.matrix_key);
    let stale = format!("b3-{}", "ff".repeat(32));
    let begin = begin_helper_obligation_report_to(
        &plan.run_key,
        &entry.task_id,
        &helper,
        &stale,
        temp.path(),
    )
    .expect_err("stale begin frame");
    assert!(
        begin
            .to_string()
            .contains("helper_frame_task_digest_mismatch")
    );
    let terminal = write_helper_obligation_report_to(
        &plan.run_key,
        &entry.task_id,
        &helper,
        &stale,
        HelperObligationOutcome::Success,
        &[],
        temp.path(),
    )
    .expect_err("stale terminal frame");
    assert!(
        terminal
            .to_string()
            .contains("helper_frame_task_digest_mismatch")
    );
    assert!(!helper_dir(temp.path(), &plan, entry).exists());
    assert!(
        !temp
            .path()
            .join("velnor")
            .join(&plan.run_key)
            .join(&entry.matrix_key)
            .join("matrix-report.json")
            .exists()
    );
}

#[test]
fn missing_or_malformed_frame_digest_has_no_fallback() {
    let (temp, plan) = staged_plan();
    let entry = &plan.matrix.include[0];
    let helper = format!("velnor-helper-{}", entry.matrix_key);
    for digest in ["", "not-a-digest", "b3-ABC"] {
        assert!(
            begin_helper_obligation_report_to(
                &plan.run_key,
                &entry.task_id,
                &helper,
                digest,
                temp.path(),
            )
            .is_err()
        );
        assert!(
            write_helper_obligation_report_to(
                &plan.run_key,
                &entry.task_id,
                &helper,
                digest,
                HelperObligationOutcome::Success,
                &[],
                temp.path(),
            )
            .is_err()
        );
    }
    assert!(!helper_dir(temp.path(), &plan, entry).exists());
}
