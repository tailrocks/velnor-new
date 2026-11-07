// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_qualification_witness::{
    WitnessRead, WitnessRequest, valid_log_time, validate_request, verify_run,
};
use crate::{OrchestratorError, internal::internal};

const MAX_LOG: usize = 4 * 1024 * 1024;
const MAX_RECORD: usize = 16 * 1024;

pub fn verify_executor_observation(
    request: &WitnessRequest,
    executor_job_id: u64,
    marker: &str,
    expected: &[u8],
    api: &dyn WitnessRead,
) -> Result<(), OrchestratorError> {
    validate_request(request)?;
    if executor_job_id == 0
        || executor_job_id == request.job_id
        || marker.is_empty()
        || marker.len() > 128
        || !marker.strip_suffix('=').is_some_and(|prefix| {
            !prefix.is_empty()
                && prefix
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        })
        || expected.len() > MAX_RECORD
    {
        return Err(invalid());
    }
    let expected =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(expected, MAX_RECORD)
            .map_err(|_| invalid())?;
    if !expected.is_object() {
        return Err(invalid());
    }
    let prefix = format!("repos/{}", request.repository);
    let run_path = format!("{prefix}/actions/runs/{}/attempts/1", request.run_id);
    let job_path = format!("{prefix}/actions/jobs/{executor_job_id}");
    let run = api.get(&run_path)?;
    verify_run(&run, request)?;
    let job = api.get(&job_path)?;
    let steps = job["steps"].as_array().ok_or_else(invalid)?;
    let selected: Vec<_> = steps
        .iter()
        .filter(|s| s["name"] == "Run rehearsal without credentials")
        .collect();
    if job["id"].as_u64() != Some(executor_job_id)
        || job["run_id"].as_u64() != Some(request.run_id)
        || job["run_attempt"].as_u64() != Some(1)
        || job["head_sha"].as_str() != Some(request.workflow_sha.as_str())
        || job["name"] != "execute"
        || job["status"] != "completed"
        || job["conclusion"] != "success"
        || selected.len() != 1
        || selected[0]["status"] != "completed"
        || selected[0]["conclusion"] != "success"
    {
        return Err(invalid());
    }
    let log = api.witness_job_log(&request.repository, executor_job_id)?;
    if log.len() > MAX_LOG || !log.ends_with(b"\n") {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&log).map_err(|_| invalid())?;
    let mut records = Vec::new();
    for line in text.lines() {
        if line.starts_with(marker) {
            return Err(invalid());
        }
        if let Some((time, body)) = line.split_once(' ')
            && let Some(record) = body.strip_prefix(marker)
        {
            if !valid_log_time(time) {
                return Err(invalid());
            }
            records.push(record);
        }
    }
    if records.len() != 1 || records[0].len() > MAX_RECORD {
        return Err(invalid());
    }
    let actual = velnor_actions_contract::strict_json::parse_strict_json_bytes(
        records[0].as_bytes(),
        MAX_RECORD,
    )
    .map_err(|_| invalid())?;
    if actual != expected || api.get(&run_path)? != run || api.get(&job_path)? != job {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> OrchestratorError {
    internal("qualification_executor_observation_invalid")
}

#[cfg(test)]
#[path = "../tests/crate_release_qualification_observation.rs"]
mod tests;
