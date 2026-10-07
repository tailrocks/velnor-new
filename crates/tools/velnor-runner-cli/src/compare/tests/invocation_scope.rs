use super::common::{TempDir, observed_pair, write_trio};
use crate::compare::{Fail, prove_for};

#[test]
fn scoped_command_does_not_prove_repository_or_run_id_absent_from_evidence() -> Result<(), String> {
    let dir = TempDir::new("scope-missing")?;
    write_trio(dir.path(), &observed_pair(), false)?;

    for (repository, run_id) in [("tailrocks/velnor-new", 7), ("other/repo", 8)] {
        match prove_for(dir.path(), repository, run_id, 1) {
            Err(Fail::ScopeUnavailable) => {}
            Err(Fail::Checker(err)) => return Err(format!("parity fixture failed: {err}")),
            Err(Fail::InvocationMismatch) => return Err("unexpected attempt mismatch".to_owned()),
            Err(Fail::Closed) => return Err("unexpected malformed evidence".to_owned()),
            Ok(_) => return Err("scope-less evidence was proven".to_owned()),
        }
    }
    Ok(())
}

#[test]
fn command_attempt_must_match_expected_observed_and_census_keys() -> Result<(), String> {
    let dir = TempDir::new("scope-attempt")?;
    write_trio(dir.path(), &observed_pair(), false)?;
    match prove_for(dir.path(), "tailrocks/velnor-new", 7, 2) {
        Err(Fail::InvocationMismatch) => Ok(()),
        Err(Fail::Checker(err)) => Err(format!("parity fixture failed: {err}")),
        Err(Fail::ScopeUnavailable) => Err("scope was checked before the attempt".to_owned()),
        Err(Fail::Closed) => Err("unexpected malformed evidence".to_owned()),
        Ok(_) => Err("wrong attempt was proven".to_owned()),
    }
}

#[test]
fn zero_run_or_attempt_and_invalid_repository_fail_closed() -> Result<(), String> {
    let dir = TempDir::new("scope-invalid")?;
    write_trio(dir.path(), &observed_pair(), false)?;
    for (repository, run_id, attempt) in [
        ("tailrocks/velnor-new", 0, 1),
        ("tailrocks/velnor-new", 7, 0),
        ("tailrocks", 7, 1),
    ] {
        if !matches!(
            prove_for(dir.path(), repository, run_id, attempt),
            Err(Fail::Closed)
        ) {
            return Err(format!(
                "invalid scope accepted: {repository} {run_id} {attempt}"
            ));
        }
    }
    Ok(())
}
