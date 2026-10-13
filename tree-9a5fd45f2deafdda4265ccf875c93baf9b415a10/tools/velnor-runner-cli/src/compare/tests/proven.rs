use std::process::ExitCode;

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
        Err(Fail::Closed) => Err("closed before checker".to_owned()),
    }
}
