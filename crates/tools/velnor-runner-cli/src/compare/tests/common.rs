//! Shared expected/observed/census fixtures for the compare cases.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use velnor_runner_core::EvidenceError;

use crate::compare::{Fail, compare_dir, prove};

pub(super) const JOB: &str = "rust_test";

pub(super) fn key_value(profile: &str, attempt: u64, job: &str) -> Value {
    json!({
        "attempt": attempt,
        "logical_job": job,
        "plan": "plan",
        "profile": profile,
        "source": "abc",
    })
}

pub(super) fn report_value(
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

pub(super) fn lane(profile: &str, artifact: &str) -> Value {
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

pub(super) fn observed_pair() -> Value {
    Value::Array(vec![
        lane("hosted", "art-hosted"),
        lane("scale-set", "art-scale-set"),
    ])
}

pub(super) struct TempDir(PathBuf);

impl TempDir {
    pub(super) fn new(label: &str) -> Result<Self, String> {
        let path = scratch(label);
        if path.exists() {
            return Err("temp path exists".to_owned());
        }
        std::fs::create_dir_all(&path).map_err(|err| err.to_string())?;
        Ok(Self(path))
    }

    pub(super) fn path(&self) -> &Path {
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

pub(super) fn scratch(label: &str) -> PathBuf {
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

pub(super) fn write_trio(dir: &Path, observed: &Value, omitted_page: bool) -> Result<(), String> {
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
        Err(Fail::InvocationMismatch) => Err("unexpected invocation mismatch".to_owned()),
        Err(Fail::ScopeUnavailable) => Err("unexpected missing invocation scope".to_owned()),
        Err(Fail::Closed) => Err("closed before checker".to_owned()),
        Ok(_) => Err(format!("prove succeeded for {expected}")),
    }
}

pub(super) fn reject_case(
    label: &str,
    observed: &Value,
    omitted_page: bool,
    expected: EvidenceError,
) -> Result<(), String> {
    let dir = TempDir::new(label)?;
    write_trio(dir.path(), observed, omitted_page)?;
    require_rejected(dir.path(), expected)
}
