// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    path::Path,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::ProbeError;

pub const CONTROL_PATH: &str = "/opt/velnor/source-proof/control";
pub const INBOX_PATH: &str = "/opt/velnor/source-proof/inbox";
pub const CHALLENGE_FILE: &str = "challenge.json";
pub const DECISION_FILE: &str = "decision.json";
pub const REQUEST_FILE: &str = "request.json";

const CHALLENGE_LIMIT: usize = 8 * 1024;
const REQUEST_LIMIT: usize = 16 * 1024;
const DECISION_LIMIT: usize = 8 * 1024;
const EVENT_LIMIT: usize = 1024 * 1024;
const DECISION_TIMEOUT: Duration = Duration::from_secs(120);
const DECISION_POLL_INTERVAL: Duration = Duration::from_millis(50);
const RUNNER_UID: u32 = 1000;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub schema_version: u32,
    #[serde(rename = "challenge")]
    pub nonce: String,
    pub reservation_id: i64,
    pub intent_id: i64,
    pub witness_request_id: i64,
    pub runner_id: i64,
    pub runner_name: String,
    pub runner_scale_set_id: i64,
    pub runner_container_id: String,
    pub dind_container_id: String,
    pub engine_id: String,
    pub docker_root_digest: String,
    pub instance_binding_digest: String,
    pub policy_generation: u64,
    pub policy_digest: String,
    pub channel_digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceRequest {
    pub schema_version: u32,
    pub challenge: String,
    pub reservation_id: i64,
    pub intent_id: i64,
    pub repository_id: u64,
    pub repository_name: String,
    pub event_name: String,
    pub event_ref: String,
    pub tested_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub job_key: String,
    pub workflow_ref: String,
    pub workflow_sha: String,
    pub base_ref: Option<String>,
    pub base_sha: Option<String>,
    pub head_ref: Option<String>,
    pub head_sha: Option<String>,
    pub pull_request_number: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub schema_version: u32,
    pub allow: bool,
    pub challenge: String,
    pub request_sha256: String,
    pub reservation_id: i64,
    pub intent_id: i64,
    pub actual_request_id: i64,
    pub run_id: u64,
    pub run_attempt: u32,
    pub job_key: String,
    pub runner_id: i64,
    pub policy_generation: u64,
    pub policy_digest: String,
    pub native_job_id: Option<String>,
    pub rest_job_id: Option<String>,
    pub deny_code: Option<String>,
}

pub fn run() -> Result<(), ProbeError> {
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

pub use validation::{canonical_json, sha256, validate_challenge, validate_decision};
