//! Required-evidence fold verdicts for skipped, failed, and missing jobs.

use super::*;

#[test]
fn skipped_check_marks_not_run() {
    let mut signals = Signals::default();
    fold_jobs(
        &[RequiredJobResult {
            job_id: "check-native".to_owned(),
            conclusion: JobConclusion::Skipped,
        }],
        &mut signals,
    );
    assert!(signals.not_run);
}

#[test]
fn required_rejects_failed_or_missing_named_check() {
    for conclusion in [JobConclusion::Missing, JobConclusion::Failure] {
        let mut signals = Signals::default();
        fold_jobs(
            &[RequiredJobResult {
                job_id: "check-ffi".to_owned(),
                conclusion,
            }],
            &mut signals,
        );
        assert!(signals.failed, "{conclusion:?}");
    }
}
