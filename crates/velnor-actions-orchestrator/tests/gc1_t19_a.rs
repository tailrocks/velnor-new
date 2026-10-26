// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_failure_report_collected() -> Result<(), Box<dyn Error>> {
    let mut session = valid_session()?;
    let ledger = rust_ledger()?;
    let native = native_ledger()?;
    provide_measured(&mut session, &ledger, &native)?;
    let report = session.finish(ChildOutcome::Failure)?;
    if !report.child_failed {
        return Err(fail("failed child reported as successful"));
    }
    report
        .validate()
        .map_err(|error| fail(&error.to_string()))?;
    if report.dependency_compilations != measured(3) {
        return Err(fail("failed child hid measured compiler work"));
    }
    Ok(())
}

fn check_report_survives_home_cleanup(report: &ObservationReport) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let observation_dir = temp.path().join("velnor-gc1").join("observations");
    std::fs::create_dir_all(&observation_dir)?;
    let home = temp.path().join("velnor-check-0");
    std::fs::create_dir(&home)?;
    let bytes = report
        .to_canonical_json()
        .map_err(|error| fail(&error.to_string()))?;
    std::fs::write(observation_dir.join("check-observation.json"), &bytes)?;
    std::fs::remove_dir_all(&home)?;
    if home.exists() {
        return Err(fail("private home survived cleanup"));
    }
    let retained = std::fs::read(observation_dir.join("check-observation.json"))?;
    let parsed: ObservationReport =
        serde_json::from_slice(&retained).map_err(|error| fail(&error.to_string()))?;
    if parsed != *report {
        return Err(fail("retained report drifted across home cleanup"));
    }
    Ok(())
}

#[test]
fn gc1_t19_preserves_contract() -> Result<(), Box<dyn Error>> {
    let first_id = new_session_id();
    let second_id = new_session_id();
    if first_id == second_id {
        return Err(fail("observation session ids collided"));
    }
    let mut session = valid_session()?;
    let ledger = rust_ledger()?;
    if ledger.dependency_compilations() != 3
        || ledger.application_compilations() != 1
        || ledger.queries() != 1
        || ledger.failed() != 2
        || ledger.interrupted() != 1
    {
        return Err(fail("rust ledger counts drifted"));
    }
    let native = native_ledger()?;
    if native.measured_native_compilations() != Some(1) || native.queries() != 1 {
        return Err(fail("native ledger counts drifted"));
    }
    provide_measured(&mut session, &ledger, &native)?;
    let report = session.finish(ChildOutcome::Success)?;
    check_summary_matches_ledgers(&report)?;
    check_json_round_trip(&report)?;
    check_failure_report_collected()?;
    check_report_survives_home_cleanup(&report)?;
    Ok(())
}

pub(crate) fn check_rejects_bad_bindings() -> Result<(), Box<dyn Error>> {
    let digest = context_digest();
    let session = new_session_id();
    for (recipe, check, id, context) in [
        ("", "gc1-t19-check", session.clone(), digest.clone()),
        ("a..b", "gc1-t19-check", session.clone(), digest.clone()),
        ("a\0b", "gc1-t19-check", session.clone(), digest.clone()),
        ("internal/lint/dev", "", session.clone(), digest.clone()),
        ("internal/lint/dev", "a..b", session.clone(), digest.clone()),
        (
            "internal/lint/dev",
            "gc1-t19-check",
            String::new(),
            digest.clone(),
        ),
        (
            "internal/lint/dev",
            "gc1-t19-check",
            "a/b".to_owned(),
            digest.clone(),
        ),
        (
            "internal/lint/dev",
            "gc1-t19-check",
            "..".to_owned(),
            digest.clone(),
        ),
        (
            "internal/lint/dev",
            "gc1-t19-check",
            session.clone(),
            String::new(),
        ),
    ] {
        if ObservationSession::begin(recipe, check, &id, &context).is_ok() {
            return Err(fail("invalid observation binding accepted"));
        }
    }
    Ok(())
}

pub(crate) fn check_rejects_partial_dimensions(
    session: &ObservationSession,
) -> Result<(), Box<dyn Error>> {
    if session
        .compiler_evidence(Some(1), None, None, None, None, "detail")
        .is_ok()
    {
        return Err(fail("partial compiler dimension accepted"));
    }
    if session
        .native_evidence(Some(1), None, None, None, "detail")
        .is_ok()
    {
        return Err(fail("partial native dimension accepted"));
    }
    if session.origin_evidence(Some(1), None, "detail").is_ok() {
        return Err(fail("partial origin dimension accepted"));
    }
    if session
        .compiler_evidence(None, None, None, None, None, "")
        .is_ok()
    {
        return Err(fail("empty unknown detail accepted"));
    }
    Ok(())
}

pub(crate) fn check_rejects_foreign_and_duplicate_evidence() -> Result<(), Box<dyn Error>> {
    let mut first = valid_session()?;
    let second = valid_session()?;
    let foreign = second.compiler_evidence(None, None, None, None, None, "detail")?;
    if first.provide_compiler(foreign).is_ok() {
        return Err(fail("foreign session evidence accepted"));
    }
    let own = first.compiler_evidence(None, None, None, None, None, "detail")?;
    first.provide_compiler(own)?;
    let repeated = first.compiler_evidence(None, None, None, None, None, "detail")?;
    if first.provide_compiler(repeated).is_ok() {
        return Err(fail("duplicate dimension accepted"));
    }
    if first.finish(ChildOutcome::Success).is_ok() {
        return Err(fail("report finished with missing dimensions"));
    }
    Ok(())
}
