//! Directory comparison for `velnor-host compare`.
//!
//! Core evidence types have no `Deserialize` impl. This parser accepts one
//! exact field set per object and builds those types by hand.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Map, Value};
use velnor_runner_core::{
    ArchiveSafety, Conclusion, EvidenceError, ExecutionKey, ExpectedExecutionSet, ExpectedItem,
    ParityProof, VerifiedExecutionReport, VerifiedJobCensus, verify_complete_results,
};

#[derive(Clone, Copy)]
pub(crate) enum Fail {
    Closed,
    Checker(EvidenceError),
}

/// Compare `expected.json`, `observed.json`, and `census.json` in `path`.
///
/// Success prints `PROVEN lanes=<n>` and returns success. A missing directory,
/// unreadable file, or unrecognized JSON field prints `NOT_PROVEN` and returns
/// a failure code with no stderr. [`velnor_runner_core::verify_complete_results`]
/// errors print `NOT_PROVEN` and the [`velnor_runner_core::EvidenceError`] on
/// stderr only.
#[must_use]
pub(crate) fn compare_dir(path: &Path) -> ExitCode {
    match prove(path) {
        Ok(proof) => {
            println!("PROVEN lanes={}", proof.lanes);
            ExitCode::SUCCESS
        }
        Err(fail) => {
            println!("NOT_PROVEN");
            if let Fail::Checker(err) = fail {
                eprintln!("{err}");
            }
            ExitCode::from(1)
        }
    }
}

pub(crate) fn prove(path: &Path) -> Result<ParityProof, Fail> {
    if !path.is_dir() {
        return Err(Fail::Closed);
    }
    let expected = expected_set(&load(path, "expected.json")?)?;
    let seen = observed(&load(path, "observed.json")?)?;
    let github = census(&load(path, "census.json")?)?;
    verify_complete_results(&expected, &seen, &github).map_err(Fail::Checker)
}

fn load(dir: &Path, name: &str) -> Result<Value, Fail> {
    let bytes = std::fs::read(dir.join(name)).map_err(|_| Fail::Closed)?;
    serde_json::from_slice(&bytes).map_err(|_| Fail::Closed)
}

fn only_object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, Fail> {
    let Some(map) = value.as_object() else {
        return Err(Fail::Closed);
    };
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return Err(Fail::Closed);
    }
    Ok(map)
}

fn field<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, Fail> {
    map.get(key).ok_or(Fail::Closed)
}

fn string(map: &Map<String, Value>, key: &str) -> Result<String, Fail> {
    field(map, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or(Fail::Closed)
}

fn boolean(map: &Map<String, Value>, key: &str) -> Result<bool, Fail> {
    field(map, key)?.as_bool().ok_or(Fail::Closed)
}

fn u64_field(map: &Map<String, Value>, key: &str) -> Result<u64, Fail> {
    field(map, key)?.as_u64().ok_or(Fail::Closed)
}

fn array(value: &Value) -> Result<&Vec<Value>, Fail> {
    value.as_array().ok_or(Fail::Closed)
}

fn execution_key(value: &Value) -> Result<ExecutionKey, Fail> {
    let map = only_object(
        value,
        &["attempt", "logical_job", "plan", "profile", "source"],
    )?;
    Ok(ExecutionKey {
        source: string(map, "source")?,
        attempt: u64_field(map, "attempt")?,
        plan: string(map, "plan")?,
        profile: string(map, "profile")?,
        logical_job: string(map, "logical_job")?,
    })
}

fn expected_item(value: &Value) -> Result<ExpectedItem, Fail> {
    let map = only_object(value, &["artifact_id", "key"])?;
    Ok(ExpectedItem {
        key: execution_key(field(map, "key")?)?,
        artifact_id: string(map, "artifact_id")?,
    })
}

fn expected_set(value: &Value) -> Result<ExpectedExecutionSet, Fail> {
    let map = only_object(value, &["items"])?;
    let mut items = Vec::new();
    for item in array(field(map, "items")?)? {
        items.push(expected_item(item)?);
    }
    Ok(ExpectedExecutionSet { items })
}

fn report(value: &Value) -> Result<VerifiedExecutionReport, Fail> {
    let map = only_object(
        value,
        &[
            "archive",
            "artifact_id",
            "cached_success",
            "conclusion",
            "key",
            "runner_known",
        ],
    )?;
    Ok(VerifiedExecutionReport {
        key: execution_key(field(map, "key")?)?,
        artifact_id: string(map, "artifact_id")?,
        conclusion: conclusion(field(map, "conclusion")?)?,
        runner_known: boolean(map, "runner_known")?,
        cached_success: boolean(map, "cached_success")?,
        archive: archive(field(map, "archive")?)?,
    })
}

fn observed(value: &Value) -> Result<Vec<VerifiedExecutionReport>, Fail> {
    let mut reports = Vec::new();
    for item in array(value)? {
        reports.push(report(item)?);
    }
    Ok(reports)
}

fn census(value: &Value) -> Result<VerifiedJobCensus, Fail> {
    let map = only_object(
        value,
        &["complete", "omitted_page", "success_on_expected_runner"],
    )?;
    let mut success_on_expected_runner = BTreeSet::new();
    for key in array(field(map, "success_on_expected_runner")?)? {
        let inserted = success_on_expected_runner.insert(execution_key(key)?);
        if !inserted {
            return Err(Fail::Closed);
        }
    }
    Ok(VerifiedJobCensus {
        complete: boolean(map, "complete")?,
        omitted_page: boolean(map, "omitted_page")?,
        success_on_expected_runner,
    })
}

fn conclusion(value: &Value) -> Result<Conclusion, Fail> {
    let Some(text) = value.as_str() else {
        return Err(Fail::Closed);
    };
    match text {
        "success" => Ok(Conclusion::Success),
        "skipped" => Ok(Conclusion::Skipped),
        "cancelled" => Ok(Conclusion::Cancelled),
        "timed_out" => Ok(Conclusion::TimedOut),
        "failed" => Ok(Conclusion::Failed),
        _ => Err(Fail::Closed),
    }
}

fn archive(value: &Value) -> Result<ArchiveSafety, Fail> {
    let Some(text) = value.as_str() else {
        return Err(Fail::Closed);
    };
    match text {
        "safe" => Ok(ArchiveSafety::Safe),
        "traversal" => Ok(ArchiveSafety::Traversal),
        "symlink" => Ok(ArchiveSafety::Symlink),
        "case_collision" => Ok(ArchiveSafety::CaseCollision),
        _ => Err(Fail::Closed),
    }
}
