// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use serde_json::json;
use velnor_runner_github::workflow_run::{
    ComparedHookSourceFacts, RepositoryTarget, UntrustedHookSourceInput, WorkflowRun,
    compare_hook_source, get_workflow_run_attempt,
};

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::error::HostError;
use crate::https::HttpsTransport;

use super::fault::SourceFault;
use super::workflow_verification::{
    VerifiedWorkflowClosure, WorkflowVerification, root_source_commit_matches,
    verify_workflow_closure,
};

const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullClaim {
    pub(crate) number: u64,
    pub(crate) base_ref: String,
    pub(crate) base_sha: String,
    pub(crate) head_ref: String,
    pub(crate) head_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceClaim {
    pub(crate) repository_id: u64,
    pub(crate) repository_name: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) event_name: String,
    pub(crate) event_ref: String,
    pub(crate) tested_sha: String,
    pub(crate) workflow_ref: String,
    pub(crate) workflow_sha: String,
    pub(crate) job_workflow_ref: String,
    pub(crate) workflow_source_sha: Option<String>,
    pub(crate) pull_request: Option<PullClaim>,
}

pub(crate) struct VerifiedClaim {
    pub(crate) compared: ComparedHookSourceFacts,
    pub(crate) closure: VerifiedWorkflowClosure,
}

pub(crate) fn verify_source_claim(
    connection: &RepositoryConnection,
    policy: &ApprovedJobAuthorityPolicy,
    claim: &SourceClaim,
    token: &str,
    deadline: Instant,
) -> Result<VerifiedClaim, SourceFault> {
    if Instant::now() >= deadline {
        return Err(SourceFault::TIMEOUT);
    }
    policy.validate_for(connection)?;
    let (owner, repository) = split_repository(&connection.repository)?;
    let target = RepositoryTarget {
        id: connection.repository_id,
        owner,
        name: repository,
    };
    let mut transport = HttpsTransport::with_deadline(GITHUB_API, deadline)?;
    let run = get_workflow_run_attempt(
        &mut transport,
        owner,
        repository,
        i64::try_from(claim.run_id).map_err(|_| HostError::Identity)?,
        i64::from(claim.run_attempt),
        token,
    )
    .map_err(SourceFault::lookup)?;
    let hook_input = hook_input(claim, &run)?;
    let compared = compare_hook_source(&mut transport, &hook_input, target, token)
        .map_err(SourceFault::compare)?;
    verify_compared_claim(claim, &compared)?;
    let closure = verify_workflow_closure(
        WorkflowVerification {
            connection,
            policy,
            claim,
            owner,
            repository,
            target,
            run: &run,
            compared: &compared,
        },
        &mut transport,
        token,
    )?;
    if Instant::now() >= deadline {
        return Err(SourceFault::TIMEOUT);
    }
    Ok(VerifiedClaim { compared, closure })
}

pub(super) fn split_repository(repository: &str) -> Result<(&str, &str), HostError> {
    let Some((owner, name)) = repository.split_once('/') else {
        return Err(HostError::Identity);
    };
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err(HostError::Identity);
    }
    Ok((owner, name))
}

fn hook_input(
    claim: &SourceClaim,
    run: &WorkflowRun,
) -> Result<UntrustedHookSourceInput, HostError> {
    let pull_request = if let Some(claimed) = &claim.pull_request {
        let number = i64::try_from(claimed.number).map_err(|_| HostError::Identity)?;
        let mut matches = run
            .pull_requests
            .iter()
            .filter(|pull_request| pull_request.number == number);
        let pull = matches.next().ok_or(HostError::Identity)?;
        if matches.next().is_some() {
            return Err(HostError::Identity);
        }
        let base = pull.base.repository.as_ref().ok_or(HostError::Identity)?;
        let head = pull.head.repository.as_ref().ok_or(HostError::Identity)?;
        let head_repository = run
            .head_repository
            .as_ref()
            .filter(|repository| repository.id == head.id)
            .ok_or(HostError::Identity)?;
        Some(json!({
            "number": number,
            "base": {
                "repository": {
                    "id": base.id,
                    "name": base.name,
                    "full_name": format!("{}/{}", run.repository.owner.login, run.repository.name)
                },
                "ref": claimed.base_ref,
                "sha": claimed.base_sha
            },
            "head": {
                "repository": {
                    "id": head.id,
                    "name": head.name,
                    "full_name": head_repository.full_name
                },
                "ref": claimed.head_ref,
                "sha": claimed.head_sha
            }
        }))
    } else {
        None
    };
    let value = json!({
        "repository_id": claim.repository_id,
        "repository": claim.repository_name,
        "run_id": claim.run_id,
        "run_attempt": claim.run_attempt,
        "event_name": claim.event_name,
        "event_ref": claim.event_ref,
        "workflow_ref": claim.workflow_ref,
        "workflow_sha": claim.workflow_sha,
        "tested_sha": claim.tested_sha,
        "job_workflow_ref": claim.job_workflow_ref,
        "pull_request": pull_request
    });
    serde_json::from_value(value).map_err(|_| HostError::Identity)
}

fn verify_compared_claim(
    claim: &SourceClaim,
    compared: &ComparedHookSourceFacts,
) -> Result<(), HostError> {
    let (run_id, attempt) = compared.run_identity();
    let (event, event_ref, tested_sha) = compared.event_source();
    let (workflow_ref, workflow_sha) = compared.workflow_source();
    if u64::try_from(run_id).ok() != Some(claim.run_id)
        || u32::try_from(attempt).ok() != Some(claim.run_attempt)
        || event != claim.event_name
        || event_ref != claim.event_ref
        || tested_sha != claim.tested_sha
        || workflow_ref != claim.workflow_ref
        || workflow_sha != claim.workflow_sha
        || !root_source_commit_matches(claim)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}
