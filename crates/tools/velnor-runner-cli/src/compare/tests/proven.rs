use std::process::ExitCode;

use serde_json::json;

use super::super::{Fail, compare_dir, prove};
use super::common::{TempDir, observed_pair, scratch, write_trio};

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
        Err(Fail::InvocationMismatch) => Err("unexpected invocation mismatch".to_owned()),
        Err(Fail::ScopeUnavailable) => Err("unexpected missing invocation scope".to_owned()),
        Err(Fail::Closed) => Err("closed before checker".to_owned()),
    }
}

#[test]
fn empty_expected_set_is_not_reported_as_proven() -> Result<(), String> {
    let dir = TempDir::new("empty-expected")?;
    for (name, value) in [
        ("expected.json", json!({ "items": [] })),
        ("observed.json", json!([])),
        (
            "census.json",
            json!({
                "complete": true,
                "omitted_page": false,
                "success_on_expected_runner": [],
            }),
        ),
    ] {
        std::fs::write(dir.path().join(name), value.to_string()).map_err(|err| err.to_string())?;
    }
    if compare_dir(dir.path()) != ExitCode::from(1) {
        return Err("empty expected set was reported as proven".to_owned());
    }
    match prove(dir.path()) {
        Err(Fail::Checker(velnor_runner_core::EvidenceError::NotProven("empty_expected_set"))) => {
            Ok(())
        }
        Err(Fail::Checker(err)) => Err(format!("unexpected evidence failure: {err}")),
        Err(Fail::InvocationMismatch) => Err("unexpected attempt mismatch".to_owned()),
        Err(Fail::ScopeUnavailable) => Err("unexpected missing invocation scope".to_owned()),
        Err(Fail::Closed) => Err("empty set rejected before core rule".to_owned()),
        Ok(_) => Err("empty expected set was proven".to_owned()),
    }
}
