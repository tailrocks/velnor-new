//! Do a check of the host decision before the official Runner starts job handlers.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use velnor_resource_probe::ProbeError;

pub(crate) const CONTROL_PATH: &str = "/opt/velnor/source-proof/control";
pub(crate) const INBOX_PATH: &str = "/opt/velnor/source-proof/inbox";
pub(crate) const CHALLENGE_FILE: &str = "challenge.json";
pub(crate) const DECISION_FILE: &str = "decision.json";
pub(crate) const REQUEST_FILE: &str = "request.json";

const CHALLENGE_LIMIT: usize = 8 * 1024;
const REQUEST_LIMIT: usize = 16 * 1024;
const DECISION_LIMIT: usize = 8 * 1024;
const EVENT_LIMIT: usize = 1024 * 1024;
const DECISION_TIMEOUT: Duration = Duration::from_secs(120);
const DECISION_POLL_INTERVAL: Duration = Duration::from_millis(50);
const RUNNER_UID: u32 = 1000;

/// One challenge that the host made for one prepared Runner.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Challenge {
    pub(crate) schema_version: u32,
    #[serde(rename = "challenge")]
    pub(crate) nonce: String,
    pub(crate) reservation_id: i64,
    pub(crate) intent_id: i64,
    pub(crate) witness_request_id: i64,
    pub(crate) runner_id: i64,
    pub(crate) runner_name: String,
    pub(crate) runner_scale_set_id: i64,
    pub(crate) runner_container_id: String,
    pub(crate) dind_container_id: String,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) instance_binding_digest: String,
    pub(crate) policy_generation: u64,
    pub(crate) policy_digest: String,
    pub(crate) channel_digest: String,
}

/// One set of permitted Runner context values.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceRequest {
    pub(crate) schema_version: u32,
    pub(crate) challenge: String,
    pub(crate) reservation_id: i64,
    pub(crate) intent_id: i64,
    pub(crate) repository_id: u64,
    pub(crate) repository_name: String,
    pub(crate) event_name: String,
    pub(crate) event_ref: String,
    pub(crate) tested_sha: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) job_key: String,
    pub(crate) workflow_ref: String,
    pub(crate) workflow_sha: String,
    pub(crate) base_ref: Option<String>,
    pub(crate) base_sha: Option<String>,
    pub(crate) head_ref: Option<String>,
    pub(crate) head_sha: Option<String>,
    pub(crate) pull_request_number: Option<u64>,
}

/// One host decision for the exact request bytes.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Decision {
    pub(crate) schema_version: u32,
    pub(crate) allow: bool,
    pub(crate) challenge: String,
    pub(crate) request_sha256: String,
    pub(crate) reservation_id: i64,
    pub(crate) intent_id: i64,
    pub(crate) actual_request_id: i64,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) job_key: String,
    pub(crate) runner_id: i64,
    pub(crate) policy_generation: u64,
    pub(crate) policy_digest: String,
    pub(crate) native_job_id: Option<String>,
    pub(crate) rest_job_id: Option<String>,
    pub(crate) deny_code: Option<String>,
}

/// Read one request channel. Return only after exact host approval.
pub(crate) fn run() -> Result<(), ProbeError> {
    if rustix::process::geteuid().as_raw() != RUNNER_UID {
        return Err(ProbeError::Invalid("job_hook_uid"));
    }
    let control = channel::open_trusted_directory(CONTROL_PATH, 0, false)?;
    let inbox = channel::open_trusted_directory(INBOX_PATH, RUNNER_UID, true)?;
    let challenge: Challenge =
        channel::read_json_at(&control, CHALLENGE_FILE, 0, CHALLENGE_LIMIT, true)?;
    validation::validate_challenge(&challenge)?;

    let started = Instant::now();
    let deadline = started
        .checked_add(DECISION_TIMEOUT)
        .ok_or(ProbeError::Overflow("job_hook_deadline"))?;
    let request = context::request_from_environment(&challenge)?;
    let request_bytes = validation::canonical_json(&request, REQUEST_LIMIT, "job_hook_request")?;
    let request_sha256 = validation::sha256(&request_bytes);
    channel::write_request_at(
        &inbox,
        Path::new(INBOX_PATH),
        &request_bytes,
        &challenge.nonce,
    )?;

    let decision = channel::wait_for_decision(&control, Path::new(CONTROL_PATH), deadline)?;
    validation::validate_decision(&challenge, &request, &decision, &request_sha256)
}

mod channel;
mod context;
mod validation;

#[cfg(test)]
#[path = "job_hook_tests.rs"]
mod tests;
