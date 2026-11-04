// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::parse_strict_json;
use velnor_actions_contract::workflow::UnitLedgerV1;

use velnor_actions_contract::HelperError;
use velnor_actions_contract::workflow::group_ledger::{
    GROUP_LEDGER_FILENAME as EMIT_FILENAME, GROUP_LEDGERS_FILENAME, JobStatus,
    assemble_group_ledgers_to, emit_group_ledger_to,
};

use super::support::{self as gc1_t54_support, RUN_KEY};

fn fail(problem: &str) -> HelperError {
    HelperError::internal(problem)
}

fn stage_ledger(
    run_dir: &std::path::Path,
    artifact: &str,
    ledger_bytes: &[u8],
) -> Result<(), HelperError> {
    let dir = run_dir.join("reports").join(artifact);
    std::fs::create_dir_all(&dir).map_err(|err| fail(&err.to_string()))?;
    std::fs::write(dir.join(EMIT_FILENAME), ledger_bytes).map_err(|err| fail(&err.to_string()))?;
    Ok(())
}

#[test]
fn assemble_matches_merge_shape() -> Result<(), HelperError> {
    let first = gc1_t54_support::build(JobStatus::Success)?;
    let first_ledger = emit_group_ledger_to(
        &first.anchor,
        &first.manifest_path,
        &first.run_dir,
        RUN_KEY,
        &first.emit_env,
    )?;
    let second = gc1_t54_support::build_in_group(JobStatus::Success, "t54-other-group")?;
    let second_ledger = emit_group_ledger_to(
        &second.anchor,
        &second.manifest_path,
        &second.run_dir,
        RUN_KEY,
        &second.emit_env,
    )?;
    let first_bytes = std::fs::read(&first_ledger).map_err(|err| fail(&err.to_string()))?;
    let second_bytes = std::fs::read(&second_ledger).map_err(|err| fail(&err.to_string()))?;
    stage_ledger(&first.run_dir, "zzz-artifact", &first_bytes)?;
    stage_ledger(&first.run_dir, "aaa-artifact", &second_bytes)?;
    let out = assemble_group_ledgers_to(&first.anchor, &first.run_dir)?;
    assert_eq!(out, first.run_dir.join(GROUP_LEDGERS_FILENAME));
    let text = std::fs::read_to_string(&out).map_err(|err| fail(&err.to_string()))?;
    let value = parse_strict_json(&text).map_err(|_| fail("unparsable_assembly"))?;
    let entries = value.as_array().ok_or_else(|| fail("assembly_not_array"))?;
    assert_eq!(entries.len(), 2);
    let keys: Vec<&str> = entries
        .iter()
        .map(|entry| {
            entry
                .get("job")
                .and_then(|job| job.get("job_key"))
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| fail("assembly_missing_job_key"))
        })
        .collect::<Result<_, _>>()?;
    let sorted = {
        let mut ordered = keys.clone();
        ordered.sort_unstable();
        ordered
    };
    assert_eq!(keys, sorted, "assembly sorted by job.job_key");
    assert_ne!(keys[0], keys[1], "distinct job keys prove the sort");
    for entry in entries {
        let line = serde_json::to_string(entry).map_err(|_| fail("entry_line"))?;
        UnitLedgerV1::parse_json(&line).map_err(|_| fail("entry_invalid"))?;
    }
    Ok(())
}

#[test]
fn empty_reports_assemble_to_empty_array() -> Result<(), HelperError> {
    let fixture = gc1_t54_support::build(JobStatus::Success)?;
    let out = assemble_group_ledgers_to(&fixture.anchor, &fixture.run_dir)?;
    let text = std::fs::read_to_string(&out).map_err(|err| fail(&err.to_string()))?;
    assert_eq!(text, "[]");
    Ok(())
}

#[test]
fn corrupt_entry_fails_loudly() -> Result<(), HelperError> {
    let fixture = gc1_t54_support::build(JobStatus::Success)?;
    let dir = fixture.run_dir.join("reports").join("bad-artifact");
    std::fs::create_dir_all(&dir).map_err(|err| fail(&err.to_string()))?;
    std::fs::write(dir.join(EMIT_FILENAME), b"not a ledger")
        .map_err(|err| fail(&err.to_string()))?;
    match assemble_group_ledgers_to(&fixture.anchor, &fixture.run_dir) {
        Ok(_) => Err(fail("corrupt ledger must fail loudly")),
        Err(err) => {
            assert!(
                err.to_string().contains("unparsable_group_ledger"),
                "unexpected error: {err}"
            );
            Ok(())
        }
    }
}
