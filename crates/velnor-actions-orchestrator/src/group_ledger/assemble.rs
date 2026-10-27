// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::parse_strict_json;
use velnor_actions_contract::workflow::UnitLedgerV1;

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

pub const ASSEMBLE_GROUP_LEDGERS_OP: &str = "assemble-group-ledgers-v1";

pub(super) const GROUP_LEDGERS_FILENAME: &str = "group-ledgers.json";

const MAX_LEDGER_JSON_BYTES: u64 = 1 << 20;
const MAX_ASSEMBLED_JSON_BYTES: u64 = 4 << 20;

/// Read every staged matrix group ledger and assemble the run ledger.
///
/// Scans `run_dir/reports/*/group-ledger.json` (one level: each downloaded
/// matrix artifact contributes its `group-ledger.json` at the artifact
/// root), parses each entry as a validated `UnitLedgerV1`, sorts by
/// `job.job_key`, and writes the canonical JSON array to
/// `run_dir/group-ledgers.json` — the exact shape
/// `read_group_ledgers` consumes.
pub fn assemble_group_ledgers() -> Result<PathBuf, OrchestratorError> {
    let run_key = crate::internal_request::resolve_run_key(None)?;
    let runner_temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let run_dir = Path::new(&runner_temp).join("velnor").join(&run_key);
    assemble_group_ledgers_to(&runner_temp, &run_dir)
}

pub(crate) fn assemble_group_ledgers_to(
    anchor: &Path,
    run_dir: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let reports_dir = run_dir.join("reports");
    let mut ledgers: Vec<(String, Vec<u8>)> = Vec::new();
    let entries = match fs::read_dir(&reports_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return write_assembled(anchor, run_dir, &Vec::new());
        }
        Err(err) => {
            return Err(OrchestratorError::io(
                reports_dir.display().to_string(),
                err.to_string(),
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|err| {
            OrchestratorError::io(reports_dir.display().to_string(), err.to_string())
        })?;
        if entry.file_type().map_or(true, |kind| kind.is_symlink()) {
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let candidate = entry.path().join(super::emit::GROUP_LEDGER_FILENAME);
        let text =
            match crate::retrieve_reports::read_staged_text(&candidate, MAX_LEDGER_JSON_BYTES) {
                Ok(text) => text,
                Err("missing") => continue,
                Err(kind) => {
                    return Err(internal(&format!(
                        "{kind}_group_ledger:{}",
                        entry.file_name().to_string_lossy()
                    )));
                }
            };
        let value = parse_strict_json(&text).map_err(|_| {
            internal(&format!(
                "unparsable_group_ledger:{}",
                entry.file_name().to_string_lossy()
            ))
        })?;
        let ledger: UnitLedgerV1 = serde_json::from_value(value).map_err(|_| {
            internal(&format!(
                "unparsable_group_ledger:{}",
                entry.file_name().to_string_lossy()
            ))
        })?;
        ledger.validate().map_err(internal_contract)?;
        let canonical = ledger.to_canonical_bytes().map_err(internal_contract)?;
        ledgers.push((ledger.job.job_key.clone(), canonical));
    }
    ledgers.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let ordered: Vec<serde_json::Value> = ledgers
        .into_iter()
        .map(|(_, bytes)| {
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .map_err(|_| internal("unparsable_group_ledger"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    write_assembled(anchor, run_dir, &ordered)
}

fn write_assembled(
    anchor: &Path,
    run_dir: &Path,
    ordered: &[serde_json::Value],
) -> Result<PathBuf, OrchestratorError> {
    let bytes =
        velnor_actions_contract::canonical_json_bytes(ordered).map_err(internal_contract)?;
    if bytes.len() as u64 > MAX_ASSEMBLED_JSON_BYTES {
        return Err(internal("oversize_group_ledgers"));
    }
    crate::exclusive_write::create_dir_no_symlink(anchor, run_dir)?;
    let out_path = run_dir.join(GROUP_LEDGERS_FILENAME);
    // Like every other staged write, assembly refuses to overwrite: each job
    // attempt runs on a fresh runner, so an existing file means a genuine
    // double-emit that must fail loudly instead of masking evidence.
    crate::exclusive_write::write_exclusive(&out_path, &bytes, "group_ledgers")?;
    Ok(out_path)
}

// gc1-t54 covers group-ledger assembly: emitted ledgers from two matrix
// artifacts assemble into the `group-ledgers.json` shape `read_group_ledgers`
// consumes (array sorted by `job.job_key`), an empty reports dir assembles to
// `[]`, and a corrupt entry fails loudly.
#[cfg(test)]
mod gc1_t54 {
    use velnor_actions_contract::parse_strict_json;
    use velnor_actions_contract::workflow::UnitLedgerV1;

    use super::super::emit::emit_group_ledger_to;
    use super::super::emit::{GROUP_LEDGER_FILENAME as EMIT_FILENAME, JobStatus};
    use super::super::gc1_t54_support::{self, RUN_KEY};
    use super::{GROUP_LEDGERS_FILENAME, assemble_group_ledgers_to};

    fn fail(problem: &str) -> crate::OrchestratorError {
        crate::internal::internal(problem)
    }

    fn stage_ledger(
        run_dir: &std::path::Path,
        artifact: &str,
        ledger_bytes: &[u8],
    ) -> Result<(), crate::OrchestratorError> {
        let dir = run_dir.join("reports").join(artifact);
        std::fs::create_dir_all(&dir).map_err(|err| fail(&err.to_string()))?;
        std::fs::write(dir.join(EMIT_FILENAME), ledger_bytes)
            .map_err(|err| fail(&err.to_string()))?;
        Ok(())
    }

    #[test]
    fn assemble_matches_merge_shape() -> Result<(), crate::OrchestratorError> {
        let first = gc1_t54_support::build(JobStatus::Success)?;
        let first_ledger = emit_group_ledger_to(
            &first.anchor,
            &first.manifest_path,
            &first.run_dir,
            RUN_KEY,
            &first.emit_env,
        )?;
        let mut second = gc1_t54_support::build(JobStatus::Success)?;
        second.emit_env.job_key = "t54-group-a-write".to_owned();
        let second_ledger = emit_group_ledger_to(
            &second.anchor,
            &second.manifest_path,
            &second.run_dir,
            RUN_KEY,
            &second.emit_env,
        )?;
        // Stage in reverse job-key order to prove assemble sorts.
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
    fn empty_reports_assemble_to_empty_array() -> Result<(), crate::OrchestratorError> {
        let fixture = gc1_t54_support::build(JobStatus::Success)?;
        let out = assemble_group_ledgers_to(&fixture.anchor, &fixture.run_dir)?;
        let text = std::fs::read_to_string(&out).map_err(|err| fail(&err.to_string()))?;
        assert_eq!(text, "[]");
        Ok(())
    }

    #[test]
    fn corrupt_entry_fails_loudly() -> Result<(), crate::OrchestratorError> {
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
}
