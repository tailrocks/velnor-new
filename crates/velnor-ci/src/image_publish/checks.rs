// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use super::gate::{Identity, MAIN_REF, Policy, REPOSITORY, REPOSITORY_ID};
use crate::error::CiError;

pub const ARTIFACT_PREFIX: &str = "velnor-job-image-candidate-";
pub const CI_WORKFLOW: &str = ".github/workflows/ci.yml";
pub const EMPTY_LISTING_RETRY_SECONDS: [u64; 5] = [3, 6, 12, 24, 45];

fn fail<T>(message: &str) -> Result<T, CiError> {
    Err(CiError::new(message))
}

#[must_use]
pub fn repository_path() -> String {
    format!("repos/{REPOSITORY}")
}

#[must_use]
pub fn tip_path() -> String {
    format!("repos/{REPOSITORY}/commits/main")
}

#[must_use]
pub fn artifacts_path(ci: CiRun) -> String {
    format!(
        "repos/{REPOSITORY}/actions/runs/{}/artifacts?per_page=100",
        ci.run_id
    )
}

#[must_use]
pub fn ci_path(source: &str) -> String {
    format!(
        "repos/{REPOSITORY}/actions/workflows/ci.yml/runs?head_sha={source}&branch=main&event=push&per_page=100"
    )
}

pub fn repository(value: &Value) -> Result<(), CiError> {
    if value["full_name"] == REPOSITORY && value["id"].as_u64() == Some(REPOSITORY_ID) {
        Ok(())
    } else {
        fail("GitHub repository ID is not the same as the canonical repository ID.")
    }
}

pub fn main_tip(value: &Value, source: &str) -> Result<(), CiError> {
    if value["sha"] == source {
        Ok(())
    } else {
        fail("Source SHA is not the current main commit.")
    }
}

pub fn artifact(response: &Value, identity: &Identity, ci: CiRun) -> Result<(), CiError> {
    let Some(listed) = response["artifacts"].as_array() else {
        return fail("Candidate artifact response has an incorrect format.");
    };
    let name = format!("{ARTIFACT_PREFIX}{}", ci.run_attempt);
    let mut matches = listed.iter().filter(|item| item["name"] == name);
    let (Some(found), None) = (matches.next(), matches.next()) else {
        return fail("This workflow run must have one candidate artifact.");
    };
    let run = &found["workflow_run"];
    if found["id"].as_u64() != Some(identity.artifact_id)
        || found["expired"] != Value::Bool(false)
        || run["id"].as_u64() != Some(ci.run_id)
        || run["head_sha"] != identity.source
        || run["head_branch"] != "main"
    {
        return fail("Candidate artifact ID, expiry, or workflow run is incorrect.");
    }
    Ok(())
}

pub fn ci_listing(response: &Value) -> Result<&[Value], CiError> {
    response["workflow_runs"].as_array().map_or_else(
        || fail("Canonical CI run response has an incorrect format."),
        |runs| Ok(runs.as_slice()),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CiRun {
    pub run_id: u64,
    pub run_attempt: u64,
}

fn canonical(run: &Value, source: &str) -> Option<(u64, u64, u64)> {
    let accepted = run["path"] == CI_WORKFLOW
        && run["event"] == "push"
        && run["head_branch"] == "main"
        && run["head_sha"] == source
        && run["head_repository"]["full_name"] == REPOSITORY;
    accepted
        .then(|| {
            Some((
                run["run_number"].as_u64()?,
                run["id"].as_u64()?,
                run["run_attempt"].as_u64()?,
            ))
        })
        .flatten()
}

pub fn ci_run(runs: &[Value], source: &str) -> Result<CiRun, CiError> {
    let mut found: Vec<(u64, u64, u64)> = runs
        .iter()
        .filter_map(|run| canonical(run, source))
        .collect();
    found.sort_unstable();
    let Some(&(number, id, attempt)) = found.last() else {
        return fail("No canonical main CI push run exists for this source.");
    };
    let latest = runs
        .iter()
        .find(|run| run["id"].as_u64() == Some(id))
        .unwrap_or(&Value::Null);
    let alone = found.iter().filter(|run| run.0 == number).count() == 1;
    if !alone || latest["status"] != "completed" || latest["conclusion"] != "success" {
        return fail("The latest canonical main CI run has no `success` conclusion.");
    }
    Ok(CiRun {
        run_id: id,
        run_attempt: attempt,
    })
}

#[must_use]
pub fn attestation_args(path: &str, source: &str, policy: &Policy) -> Vec<String> {
    [
        "attestation",
        "verify",
        path,
        "--repo",
        REPOSITORY,
        "--source-ref",
        MAIN_REF,
        "--source-digest",
        source,
        "--signer-workflow",
        policy.workflow(),
        "--signer-digest",
        source,
        "--format",
        "json",
    ]
    .iter()
    .map(|part| (*part).to_owned())
    .collect()
}

pub fn attestation(output: &Value, source: &str, policy: &Policy) -> Result<(), CiError> {
    let Some(verified) = output.as_array().filter(|items| !items.is_empty()) else {
        return fail("Attestation verification output has no records.");
    };
    for item in verified {
        let certificate = &item["verificationResult"]["signature"]["certificate"];
        let identifier = &certificate["sourceRepositoryIdentifier"];
        if identifier.is_null() {
            return fail("Attestation certificate has no repository identifier.");
        }
        let id = REPOSITORY_ID.to_string();
        if identifier.as_u64() != Some(REPOSITORY_ID) && identifier.as_str() != Some(id.as_str()) {
            return fail("Attestation certificate repository ID is incorrect.");
        }
        let sound = certificate["githubWorkflowRepository"] == REPOSITORY
            && certificate["githubWorkflowSHA"] == source
            && certificate["githubWorkflowRef"] == MAIN_REF
            && certificate["githubWorkflowTrigger"] == policy.event()
            && certificate["sourceRepositoryRef"] == MAIN_REF
            && certificate["sourceRepositoryDigest"] == source;
        if !sound {
            return fail("Attestation certificate source or workflow identity is incorrect.");
        }
    }
    Ok(())
}
