// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_github::GitHubAdmissionRead;
use crate::{OrchestratorError, internal::internal};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Cursor, Read};

const MAX_LOG: usize = 4 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 1024 * 1024;
const MAX_OBSERVATION: u64 = 16 * 1024;
const MARKER: &str = "VELNOR_REHEARSAL_WITNESS ";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WitnessRequest {
    pub repository: String,
    pub repository_id: u64,
    pub workflow_sha: String,
    pub workflow_path: String,
    pub run_id: u64,
    pub run_attempt: u32,
    pub job_id: u64,
    pub environment: String,
    pub reviewer_id: u64,
    pub output_bundle: bool,
    pub metadata: Option<velnor_actions_contract::config::ReleaseRehearsalMetadataConfig>,
    pub source_sha: Option<String>,
    pub script: String,
    pub source_sha256: String,
    pub candidate_pr: u64,
    pub candidate_sha: String,
}

pub trait WitnessRead: GitHubAdmissionRead {
    fn witness_job_log(&self, repository: &str, job_id: u64) -> Result<Vec<u8>, OrchestratorError>;
    fn artifact_bytes(
        &self,
        repository: &str,
        artifact_id: u64,
        expected_size: u64,
        expected_sha256: &str,
    ) -> Result<Vec<u8>, OrchestratorError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessEvidence {
    pub job_id: u64,
    pub artifact_id: u64,
    pub artifact_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WitnessRecord {
    schema: u32,
    observation: String,
    repository: String,
    workflow_sha: String,
    run_id: String,
    run_attempt: String,
    script: String,
    source_sha256: String,
    environment: String,
    reviewer_id: String,
    unverified_candidate_pr: String,
    unverified_candidate_sha: String,
}

pub fn verify_witness(
    request: &WitnessRequest,
    api: &dyn WitnessRead,
) -> Result<WitnessEvidence, OrchestratorError> {
    validate_request(request)?;
    let prefix = format!("repos/{}", request.repository);
    let run_path = format!("{prefix}/actions/runs/{}/attempts/1", request.run_id);
    let run = api.get(&run_path)?;
    verify_run(&run, request)?;
    let job_path = format!("{prefix}/actions/jobs/{}", request.job_id);
    let job = api.get(&job_path)?;
    verify_job(&job, request)?;
    let log = api.witness_job_log(&request.repository, request.job_id)?;
    let (artifact_id, digest) = marker(&log, request, MARKER)?;
    let artifact_path = format!("{prefix}/actions/artifacts/{artifact_id}");
    let artifact = api.get(&artifact_path)?;
    let size = verify_artifact(
        &artifact,
        request,
        artifact_id,
        &digest,
        "rehearsal-observation",
        MAX_ARCHIVE,
    )?;
    let archive = api.artifact_bytes(&request.repository, artifact_id, size, &digest)?;
    let observation = read_observation(&archive, size, &digest)?;
    verify_observation(&observation, request)?;
    if api.get(&run_path)? != run
        || api.get(&job_path)? != job
        || api.get(&artifact_path)? != artifact
    {
        return Err(invalid());
    }
    Ok(WitnessEvidence {
        job_id: request.job_id,
        artifact_id,
        artifact_sha256: digest,
    })
}

pub(crate) fn validate_request(r: &WitnessRequest) -> Result<(), OrchestratorError> {
    let config = velnor_actions_contract::config::ReleaseRehearsalConfig {
        script: r.script.clone(),
        environment: r.environment.clone(),
        reviewer_id: r.reviewer_id,
        output_bundle: r.output_bundle,
        metadata: r.metadata.clone(),
    };
    if !velnor_actions_contract::vcs::is_safe_github_repository(&r.repository)
        || r.repository_id == 0
        || r.run_id == 0
        || r.run_attempt != 1
        || r.job_id == 0
        || r.reviewer_id == 0
        || r.candidate_pr == 0
        || r.workflow_path != ".github/workflows/release-rehearsal.yml"
        || r.environment.is_empty()
        || r.script.is_empty()
        || !hex(&r.workflow_sha, 40)
        || !hex(&r.candidate_sha, 40)
        || !hex(&r.source_sha256, 64)
        || r.metadata.is_some() != r.source_sha.is_some()
        || r.source_sha.as_ref().is_some_and(|sha| !hex(sha, 40))
        || config.validate("rehearsal").is_err()
    {
        return Err(invalid());
    }
    Ok(())
}

fn hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) fn verify_run(run: &Value, r: &WitnessRequest) -> Result<(), OrchestratorError> {
    if run["id"].as_u64() != Some(r.run_id)
        || run["run_attempt"].as_u64() != Some(1)
        || run["head_sha"].as_str() != Some(r.workflow_sha.as_str())
        || run["repository"]["id"].as_u64() != Some(r.repository_id)
        || run["head_repository"]["id"].as_u64() != Some(r.repository_id)
        || run["path"].as_str() != Some(r.workflow_path.as_str())
        || run["event"] != "workflow_dispatch"
        || run["status"] != "completed"
        || run["conclusion"] != "success"
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn verify_job(job: &Value, r: &WitnessRequest) -> Result<(), OrchestratorError> {
    let steps = job["steps"].as_array().ok_or_else(invalid)?;
    let marker_steps: Vec<&Value> = steps
        .iter()
        .filter(|step| step["name"] == "Bind observation artifact")
        .collect();
    if job["id"].as_u64() != Some(r.job_id)
        || job["run_id"].as_u64() != Some(r.run_id)
        || job["run_attempt"].as_u64() != Some(u64::from(r.run_attempt))
        || job["head_sha"].as_str() != Some(r.workflow_sha.as_str())
        || job["name"] != "witness"
        || job["status"] != "completed"
        || job["conclusion"] != "success"
        || marker_steps.len() != 1
        || marker_steps[0]["status"] != "completed"
        || marker_steps[0]["conclusion"] != "success"
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn marker(
    log: &[u8],
    r: &WitnessRequest,
    marker: &str,
) -> Result<(u64, String), OrchestratorError> {
    if log.len() > MAX_LOG {
        return Err(invalid());
    }
    let text = std::str::from_utf8(log).map_err(|_| invalid())?;
    if !text.ends_with('\n') {
        return Err(invalid());
    }
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.starts_with(marker) {
            return Err(invalid());
        }
        if let Some((time, body)) = line.split_once(' ')
            && body.starts_with(marker)
        {
            if !valid_log_time(time) {
                return Err(invalid());
            }
            lines.push(body);
        }
    }
    if lines.len() != 1 {
        return Err(invalid());
    }
    let fields: Vec<&str> = lines[0].split(' ').collect();
    if fields.len() != 5
        || fields[0] != marker.trim_end()
        || fields[1] != format!("run={}", r.run_id)
        || fields[2] != "attempt=1"
    {
        return Err(invalid());
    }
    let id_text = fields[3].strip_prefix("artifact=").ok_or_else(invalid)?;
    let id = id_text
        .parse::<u64>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(invalid)?;
    if id_text != id.to_string() {
        return Err(invalid());
    }
    let digest = fields[4]
        .strip_prefix("sha256=")
        .filter(|v| hex(v, 64))
        .ok_or_else(invalid)?;
    Ok((id, digest.to_owned()))
}

pub(crate) fn valid_log_time(value: &str) -> bool {
    let Some(base) = value.strip_suffix('Z') else {
        return false;
    };
    let date = base.get(..19).unwrap_or_default();
    let bytes = date.as_bytes();
    if bytes.len() != 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, b)| matches!(i, 4 | 7 | 10 | 13 | 16) || b.is_ascii_digit())
    {
        return false;
    }
    let suffix = base.get(19..).unwrap_or_default();
    suffix.is_empty()
        || suffix.strip_prefix('.').is_some_and(|fraction| {
            !fraction.is_empty()
                && fraction.len() <= 9
                && fraction.bytes().all(|b| b.is_ascii_digit())
        })
}

pub(crate) fn verify_artifact(
    value: &Value,
    r: &WitnessRequest,
    id: u64,
    digest: &str,
    name: &str,
    limit: u64,
) -> Result<u64, OrchestratorError> {
    let size = value["size_in_bytes"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= limit)
        .ok_or_else(invalid)?;
    if value["id"].as_u64() != Some(id)
        || value["name"] != name
        || value["expired"] != false
        || value["digest"].as_str() != Some(format!("sha256:{digest}").as_str())
        || value["workflow_run"]["id"].as_u64() != Some(r.run_id)
        || value["workflow_run"]["repository_id"].as_u64() != Some(r.repository_id)
        || value["workflow_run"]["head_repository_id"].as_u64() != Some(r.repository_id)
        || value["workflow_run"]["head_sha"].as_str() != Some(r.workflow_sha.as_str())
    {
        return Err(invalid());
    }
    Ok(size)
}

fn read_observation(
    archive: &[u8],
    size: u64,
    digest: &str,
) -> Result<WitnessRecord, OrchestratorError> {
    if archive.len() as u64 != size || crate::generator::sha256_hex(archive) != digest {
        return Err(invalid());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|_| invalid())?;
    if zip.len() != 1 {
        return Err(invalid());
    }
    let file = zip.by_index(0).map_err(|_| invalid())?;
    if file.name_raw() != b"rehearsal-observation.json"
        || !file.is_file()
        || file.is_symlink()
        || file.encrypted()
        || file.enclosed_name().is_none()
        || file.size() > MAX_OBSERVATION
    {
        return Err(invalid());
    }
    let mut data = Vec::new();
    file.take(MAX_OBSERVATION + 1)
        .read_to_end(&mut data)
        .map_err(|_| invalid())?;
    let json = velnor_actions_contract::strict_json::parse_strict_json_bytes(
        &data,
        usize::try_from(MAX_OBSERVATION).map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
    serde_json::from_value(json).map_err(|_| invalid())
}

fn verify_observation(o: &WitnessRecord, r: &WitnessRequest) -> Result<(), OrchestratorError> {
    if o.schema != 1
        || o.observation != "script-exited-successfully"
        || o.repository != r.repository
        || o.workflow_sha != r.workflow_sha
        || o.run_id != r.run_id.to_string()
        || o.run_attempt != "1"
        || o.script != r.script
        || o.source_sha256 != r.source_sha256
        || o.environment != r.environment
        || o.reviewer_id != r.reviewer_id.to_string()
        || o.unverified_candidate_pr != r.candidate_pr.to_string()
        || o.unverified_candidate_sha != r.candidate_sha
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn invalid() -> OrchestratorError {
    internal("qualification_witness_invalid")
}

#[cfg(test)]
#[path = "../tests/crate_release_qualification_witness.rs"]
mod tests;
