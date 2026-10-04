use super::{expected, run_report};

#[test]
fn failed_save_preserves_failure_and_reports_actual_exact_receipt() {
    for (outcome, key, available) in [
        ("success", "source-key-publication", true),
        ("success", "", false),
        ("success", "foreign-key", false),
        ("failure", "source-key-publication", false),
    ] {
        let report = run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_SAVE_OUTCOME", "failure"),
            ("VELNOR_SOURCE_PUBLICATION_OUTCOME", outcome),
            ("VELNOR_SOURCE_PUBLICATION_MATCHED_KEY", key),
        ]);
        assert_eq!(report, expected(available, true, "CACHE_TRANSPORT_FAILED"));
    }
}

#[test]
fn native_preparation_failure_preserves_closed_stage_reason() {
    let report = run_report(&[
        ("VELNOR_SOURCE_OUTCOME", "failure"),
        ("VELNOR_SOURCE_ERROR", "PREPARATION_FAILED"),
    ]);
    assert_eq!(report, expected(false, false, "PREPARATION_FAILED"));
}
