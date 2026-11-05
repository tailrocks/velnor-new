// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::parse_strict_json;
use crate::workflow::UnitLedgerV1;

use crate::helper_error::HelperError;

pub const ASSEMBLE_GROUP_LEDGERS_OP: &str = "assemble-group-ledgers-v1";

pub const GROUP_LEDGERS_FILENAME: &str = "group-ledgers.json";

const MAX_LEDGER_JSON_BYTES: u64 = 1 << 20;
const MAX_ASSEMBLED_JSON_BYTES: u64 = 4 << 20;

pub fn assemble_group_ledgers() -> Result<PathBuf, HelperError> {
    let run_key = crate::resolve_run_key(None)?;
    let runner_temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| HelperError::internal("missing_runner_temp"))?;
    let run_dir = Path::new(&runner_temp).join("velnor").join(&run_key);
    assemble_group_ledgers_to(&runner_temp, &run_dir)
}

pub fn assemble_group_ledgers_to(anchor: &Path, run_dir: &Path) -> Result<PathBuf, HelperError> {
    let reports_dir = run_dir.join("reports");
    let mut ledgers: Vec<(String, Vec<u8>)> = Vec::new();
    let entries = match fs::read_dir(&reports_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return write_assembled(anchor, run_dir, &Vec::new());
        }
        Err(err) => {
            return Err(HelperError::io(
                reports_dir.display().to_string(),
                err.to_string(),
            ));
        }
    };
    for entry in entries {
        let entry = entry
            .map_err(|err| HelperError::io(reports_dir.display().to_string(), err.to_string()))?;
        if entry.file_type().map_or(true, |kind| kind.is_symlink()) {
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let candidate = entry.path().join(super::emit::GROUP_LEDGER_FILENAME);
        let text = match crate::staged_reads::read_staged_text(&candidate, MAX_LEDGER_JSON_BYTES) {
            Ok(text) => text,
            Err("missing") => continue,
            Err(kind) => {
                return Err(HelperError::internal(&format!(
                    "{kind}_group_ledger:{}",
                    entry.file_name().to_string_lossy()
                )));
            }
        };
        let value = parse_strict_json(&text).map_err(|_| {
            HelperError::internal(&format!(
                "unparsable_group_ledger:{}",
                entry.file_name().to_string_lossy()
            ))
        })?;
        let ledger: UnitLedgerV1 = serde_json::from_value(value).map_err(|_| {
            HelperError::internal(&format!(
                "unparsable_group_ledger:{}",
                entry.file_name().to_string_lossy()
            ))
        })?;
        ledger.validate().map_err(HelperError::from)?;
        let canonical = ledger.to_canonical_bytes().map_err(HelperError::from)?;
        ledgers.push((ledger.job.job_key.clone(), canonical));
    }
    ledgers.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let ordered: Vec<serde_json::Value> = ledgers
        .into_iter()
        .map(|(_, bytes)| {
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .map_err(|_| HelperError::internal("unparsable_group_ledger"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    write_assembled(anchor, run_dir, &ordered)
}

fn write_assembled(
    anchor: &Path,
    run_dir: &Path,
    ordered: &[serde_json::Value],
) -> Result<PathBuf, HelperError> {
    let bytes = crate::canonical_json_bytes(ordered).map_err(HelperError::from)?;
    if bytes.len() as u64 > MAX_ASSEMBLED_JSON_BYTES {
        return Err(HelperError::internal("oversize_group_ledgers"));
    }
    crate::exclusive_write::create_dir_no_symlink(anchor, run_dir)?;
    let out_path = run_dir.join(GROUP_LEDGERS_FILENAME);
    crate::exclusive_write::write_exclusive(&out_path, &bytes, "group_ledgers")?;
    Ok(out_path)
}
