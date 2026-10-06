//! Fail-closed `velnor-host compare` cases call `compare_dir` and `prove`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use velnor_runner_core::EvidenceError;

use crate::compare::{Fail, compare_dir, prove};

const JOB: &str = "rust_test";

fn key_value(profile: &str, attempt: u64, job: &str) -> Value {
    json!({
        "attempt": attempt,
        "logical_job": job,
        "plan": "plan",
        "profile": profile,
        "source": "abc",
    })
}

fn report_value(
    key: &Value,
    artifact: &str,
    conclusion: &str,
    archive: &str,
    cached: bool,
) -> Value {
    json!({
        "archive": archive,
        "artifact_id": artifact,
        "cached_success": cached,
        "conclusion": conclusion,
        "key": key,
        "runner_known": true,
    })
}

fn lane(profile: &str, artifact: &str) -> Value {
    report_value(
        &key_value(profile, 1, JOB),
        artifact,
        "success",
        "safe",
        false,
    )
}

fn expected_value() -> Value {
    json!({
        "items": [
            {"artifact_id": "art-hosted", "key": key_value("hosted", 1, JOB)},
            {"artifact_id": "art-scale-set", "key": key_value("scale-set", 1, JOB)},
        ],
    })
}

fn census_value(omitted_page: bool) -> Value {
    json!({
        "complete": true,
        "omitted_page": omitted_page,
        "success_on_expected_runner": [
            &key_value("hosted", 1, JOB),
            key_value("scale-set", 1, JOB),
        ],
    })
}

fn observed_pair() -> Value {
    Value::Array(vec![
        lane("hosted", "art-hosted"),
        lane("scale-set", "art-scale-set"),
    ])
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, String> {
        let path = scratch(label);
        if path.exists() {
            return Err("temp path exists".to_owned());
        }
        std::fs::create_dir_all(&path).map_err(|err| err.to_string())?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.0) {
            Ok(()) | Err(_) => {}
        }
    }
}

fn scratch(label: &str) -> PathBuf {
    static TICK: AtomicU64 = AtomicU64::new(0);
    let n = TICK.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("velnor-compare-{label}-{}-{n}", std::process::id()))
}

fn write_file(dir: &Path, name: &str, body: &str) -> Result<(), String> {
    std::fs::write(dir.join(name), body).map_err(|err| err.to_string())
}

fn write_value(dir: &Path, name: &str, value: &Value) -> Result<(), String> {
    write_file(dir, name, &value.to_string())
}

fn write_trio(dir: &Path, observed: &Value, omitted_page: bool) -> Result<(), String> {
    write_value(dir, "expected.json", &expected_value())?;
    write_value(dir, "observed.json", observed)?;
    write_value(dir, "census.json", &census_value(omitted_page))?;
    Ok(())
}

fn require_rejected(dir: &Path, expected: EvidenceError) -> Result<(), String> {
    if compare_dir(dir) != ExitCode::from(1) {
        return Err(format!("compare_dir succeeded for {expected}"));
    }
    match prove(dir) {
        Err(Fail::Checker(err)) if err == expected => Ok(()),
        Err(Fail::Checker(err)) => Err(format!("checker {err:?} want {expected:?}")),
        Err(Fail::Closed) => Err("closed before checker".to_owned()),
        Ok(_) => Err(format!("prove succeeded for {expected}")),
    }
}

fn reject_case(
    label: &str,
    observed: &Value,
    omitted_page: bool,
    expected: EvidenceError,
) -> Result<(), String> {
    let dir = TempDir::new(label)?;
    write_trio(dir.path(), observed, omitted_page)?;
    require_rejected(dir.path(), expected)
}

#[test]
fn missing_or_empty_directory_is_not_proven() -> Result<(), String> {
    let missing = scratch("missing");
    if missing.exists() {
        return Err("missing path exists".to_owned());
    }
    if compare_dir(&missing) != ExitCode::from(1) {
        return Err("missing dir was proven".to_owned());
    }
    let empty = TempDir::new("empty")?;
    if compare_dir(empty.path()) != ExitCode::from(1) {
        return Err("empty dir was proven".to_owned());
    }
    Ok(())
}

#[test]
fn hosted_and_scale_set_pair_is_proven() -> Result<(), String> {
    let dir = TempDir::new("proven")?;
    write_trio(dir.path(), &observed_pair(), false)?;
    if compare_dir(dir.path()) != ExitCode::SUCCESS {
        return Err("pair was not proven".to_owned());
    }
    match prove(dir.path()) {
        Ok(proof) if proof.lanes == 2 => Ok(()),
        Ok(proof) => Err(format!("lanes {}", proof.lanes)),
        Err(Fail::Checker(err)) => Err(err.to_string()),
        Err(Fail::Closed) => Err("closed before checker".to_owned()),
    }
}

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
