use super::super::RunnerLogAvailability;
use super::super::projection::sanitize_archive_with_availability;
use super::common::{RUNNER_NAME, TRACE, WORKER_NAME, archive, directory_archive, valid_archive};

#[test]
fn typed_availability_distinguishes_each_trace_presence_state() -> Result<(), String> {
    let cases = [
        (directory_archive("_diag")?, RunnerLogAvailability::None),
        (
            archive(&[(RUNNER_NAME, TRACE)])?,
            RunnerLogAvailability::RunnerOnly,
        ),
        (
            archive(&[(WORKER_NAME, TRACE)])?,
            RunnerLogAvailability::WorkerOnly,
        ),
        (valid_archive()?, RunnerLogAvailability::RunnerAndWorker),
    ];
    for (bytes, expected) in cases {
        let sanitized =
            sanitize_archive_with_availability(&bytes).map_err(|error| format!("{error:?}"))?;
        if sanitized.availability != expected {
            return Err(format!(
                "typed trace availability mismatch: expected {expected:?}, got {:?}",
                sanitized.availability
            ));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&sanitized.bytes).map_err(|error| error.to_string())?;
        if json["runner_log_present"]
            != matches!(
                expected,
                RunnerLogAvailability::RunnerOnly | RunnerLogAvailability::RunnerAndWorker
            )
            || json["worker_log_present"]
                != matches!(
                    expected,
                    RunnerLogAvailability::WorkerOnly | RunnerLogAvailability::RunnerAndWorker
                )
        {
            return Err("typed availability disagreed with persisted closed projection".to_owned());
        }
    }
    Ok(())
}
