use serde_json::Value;
use velnor_runner_core::EvidenceError;

use super::common::{JOB, key_value, lane, observed_pair, reject_case, report_value};

#[test]
fn duplicate_execution_is_not_proven() -> Result<(), String> {
    let observed = Value::Array(vec![
        lane("hosted", "art-hosted"),
        lane("scale-set", "art-scale-set"),
        lane("hosted", "art-hosted"),
    ]);
    reject_case(
        "duplicate",
        &observed,
        false,
        EvidenceError::DuplicateExecution,
    )
}

#[test]
fn missing_lane_is_not_proven() -> Result<(), String> {
    // Unknown key fails in `classify` as `missing_lane` before the length check.
    // An expected lane with no report is `IncompleteExecutionSet`.
    let unknown = Value::Array(vec![
        lane("hosted", "art-hosted"),
        lane("scale-set", "art-scale-set"),
        report_value(
            &key_value("hosted", 1, "other_job"),
            "art-other",
            "success",
            "safe",
            false,
        ),
    ]);
    reject_case(
        "missing-lane",
        &unknown,
        false,
        EvidenceError::NotProven("missing_lane"),
    )?;
    let absent = Value::Array(vec![lane("hosted", "art-hosted")]);
    reject_case(
        "absent-lane",
        &absent,
        false,
        EvidenceError::IncompleteExecutionSet,
    )
}

#[test]
fn wrong_attempt_is_not_proven() -> Result<(), String> {
    let observed = Value::Array(vec![
        report_value(
            &key_value("hosted", 2, JOB),
            "art-hosted",
            "success",
            "safe",
            false,
        ),
        lane("scale-set", "art-scale-set"),
    ]);
    reject_case(
        "wrong-attempt",
        &observed,
        false,
        EvidenceError::NotProven("wrong_attempt"),
    )
}

#[test]
fn swapped_artifact_is_not_proven() -> Result<(), String> {
    let observed = Value::Array(vec![
        report_value(
            &key_value("hosted", 1, JOB),
            "art-scale-set",
            "success",
            "safe",
            false,
        ),
        lane("scale-set", "art-scale-set"),
    ]);
    reject_case(
        "swapped-artifact",
        &observed,
        false,
        EvidenceError::NotProven("swapped_artifact"),
    )
}

#[test]
fn omitted_page_is_not_proven() -> Result<(), String> {
    reject_case(
        "omitted-page",
        &observed_pair(),
        true,
        EvidenceError::NotProven("omitted_page"),
    )
}

#[test]
fn unsafe_archives_are_not_proven() -> Result<(), String> {
    for archive in ["traversal", "symlink", "case_collision"] {
        let observed = Value::Array(vec![
            report_value(
                &key_value("hosted", 1, JOB),
                "art-hosted",
                "success",
                archive,
                false,
            ),
            lane("scale-set", "art-scale-set"),
        ]);
        reject_case(archive, &observed, false, EvidenceError::NotProven(archive))?;
    }
    Ok(())
}

#[test]
fn bad_conclusions_are_not_proven() -> Result<(), String> {
    for conclusion in ["skipped", "cancelled", "timed_out", "failed"] {
        let observed = Value::Array(vec![
            lane("hosted", "art-hosted"),
            report_value(
                &key_value("scale-set", 1, JOB),
                "art-scale-set",
                conclusion,
                "safe",
                false,
            ),
        ]);
        reject_case(
            conclusion,
            &observed,
            false,
            EvidenceError::NotProven("bad_conclusion"),
        )?;
    }
    Ok(())
}

#[test]
fn cached_success_is_not_proven() -> Result<(), String> {
    let observed = Value::Array(vec![
        lane("hosted", "art-hosted"),
        report_value(
            &key_value("scale-set", 1, JOB),
            "art-scale-set",
            "success",
            "safe",
            true,
        ),
    ]);
    reject_case(
        "cached",
        &observed,
        false,
        EvidenceError::NotProven("cached_success"),
    )
}
