// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use serde_json::Value;

use super::{internal, nonempty};
use crate::error::CiError;

const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

/// The runner's view of the run, as `GITHUB_*` names it.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunnerContext<'a> {
    pub repository: Option<&'a str>,
    pub git_ref: Option<&'a str>,
    pub ref_protected: Option<&'a str>,
    pub workflow_ref: Option<&'a str>,
    pub workflow_sha: Option<&'a str>,
    pub source_sha: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub run_attempt: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Cold,
    Warm,
    Third,
    UsefulDelta,
    Control,
}

impl Phase {
    fn parse(text: Option<&str>) -> Option<Self> {
        match text? {
            "cold" => Some(Self::Cold),
            "warm" => Some(Self::Warm),
            "third" => Some(Self::Third),
            "useful_delta" => Some(Self::UsefulDelta),
            "control" => Some(Self::Control),
            _ => None,
        }
    }

    const fn needs_predecessor(self) -> bool {
        matches!(self, Self::Warm | Self::Third | Self::UsefulDelta)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
struct RunRef {
    run_id: u64,
    run_attempt: u32,
}

#[derive(Debug, Serialize)]
struct Dispatch {
    campaign: String,
    phase: Phase,
    repository: String,
    default_branch: String,
    git_ref: String,
    ref_protected: bool,
    workflow_ref: String,
    workflow_sha: String,
    source_sha: String,
    run_id: u64,
    run_attempt: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    predecessor: Option<RunRef>,
}

/// The qualification dispatch of a `workflow_dispatch` run, `None` for any other event.
pub fn dispatch_for_parts(
    event_name: &str,
    payload: &Value,
    runner: &RunnerContext<'_>,
) -> Result<Option<Value>, CiError> {
    if event_name != "workflow_dispatch" {
        return Ok(None);
    }
    let inputs = &payload["inputs"];
    let text = |value: &Value| nonempty(value.as_str());
    let payload_repository = text(&payload["repository"]["full_name"])
        .ok_or_else(|| internal("missing_qualification_payload_repository"))?;
    let default_branch = text(&payload["repository"]["default_branch"])
        .ok_or_else(|| internal("missing_qualification_default_branch"))?;
    let payload_ref =
        text(&payload["ref"]).ok_or_else(|| internal("missing_qualification_payload_ref"))?;
    let repository = required(runner.repository, "repository")?;
    let git_ref = required(runner.git_ref, "ref")?;
    if payload_repository != repository || payload_ref != git_ref {
        return Err(internal("qualification_payload_context_mismatch"));
    }
    let campaign =
        text(&inputs["campaign"]).ok_or_else(|| internal("missing_qualification_campaign"))?;
    let phase = Phase::parse(inputs["phase"].as_str())
        .ok_or_else(|| internal("invalid_qualification_phase"))?;
    let predecessor = predecessor(inputs)?;
    let run_id = runner
        .run_id
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| internal("invalid_qualification_run_id"))?;
    let run_attempt = runner
        .run_attempt
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| internal("invalid_qualification_run_attempt"))?;
    let context = Dispatch {
        campaign,
        phase,
        repository,
        default_branch,
        git_ref,
        ref_protected: runner.ref_protected == Some("true"),
        workflow_ref: required(runner.workflow_ref, "workflow_ref")?,
        workflow_sha: required(runner.workflow_sha, "workflow_sha")?,
        source_sha: required(runner.source_sha, "source_sha")?,
        run_id,
        run_attempt,
        predecessor,
    };
    context.validate_shape()?;
    serde_json::to_value(&context)
        .map(Some)
        .map_err(|_| internal("invalid_qualification_context"))
}

fn predecessor(inputs: &Value) -> Result<Option<RunRef>, CiError> {
    let run_id = nonempty(inputs["predecessor_run_id"].as_str());
    let run_attempt = nonempty(inputs["predecessor_run_attempt"].as_str());
    match (run_id, run_attempt) {
        (None, None) => Ok(None),
        (Some(run_id), Some(run_attempt)) => Ok(Some(RunRef {
            run_id: run_id
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| internal("invalid_qualification_predecessor_run_id"))?,
            run_attempt: run_attempt
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| internal("invalid_qualification_predecessor_run_attempt"))?,
        })),
        _ => Err(internal("incomplete_qualification_predecessor")),
    }
}

fn required(value: Option<&str>, name: &str) -> Result<String, CiError> {
    nonempty(value).ok_or_else(|| internal(&format!("missing_qualification_{name}")))
}

impl Dispatch {
    /// The checks every qualification context passes before it is written; one code for all.
    fn validate_shape(&self) -> Result<(), CiError> {
        let valid = valid_campaign(&self.campaign)
            && valid_repository(&self.repository)
            && lower_hex_sha(&self.source_sha)
            && lower_hex_sha(&self.workflow_sha)
            && self.source_sha == self.workflow_sha
            && self.ref_protected
            && valid_branch(&self.default_branch)
            && self.git_ref == format!("refs/heads/{}", self.default_branch)
            && self.workflow_ref
                == format!("{}/{CI_WORKFLOW_PATH}@{}", self.repository, self.git_ref)
            && self.run_id != 0
            && self.run_attempt != 0
            && self.phase.needs_predecessor() == self.predecessor.is_some();
        if valid {
            Ok(())
        } else {
            Err(internal("invalid_qualification_context"))
        }
    }
}

fn valid_campaign(campaign: &str) -> bool {
    !campaign.is_empty()
        && campaign.len() <= 64
        && campaign
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && campaign
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn valid_repository(repository: &str) -> bool {
    let parts: Vec<&str> = repository.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('/')
        && !branch.ends_with('/')
        && !branch.starts_with('.')
        && !branch.ends_with('.')
        && !branch.contains("..")
        && !branch.contains("@{")
        && branch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

fn lower_hex_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
