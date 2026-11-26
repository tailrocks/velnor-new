// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use velnor_runner_github::SourceMetadata;
use velnor_runner_github::workflow_run::{
    VerifiedWorkflowSource, WorkflowRun, get_pull_request, get_workflow_run_attempt,
};

use crate::config::{ApprovedJobAuthorityPolicy, RepositoryConnection};
use crate::error::HostError;
use crate::https::HttpsTransport;
use crate::launch::execution_authorization::{
    PullClaim, SourceClaim, VerifiedWorkflowIdentity, verify_source_claim,
};
use crate::launch::steps::source::authorize_source;

const GITHUB_API: &str = "https://api.github.com";
const GITHUB_SERVER_URL: &str = "https://github.com";

#[derive(Debug, Clone, PartialEq, Eq)]
struct GrantedPull {
    number: u64,
    base_ref: String,
    head_repository_id: i64,
    head_repository: String,
}

pub(super) struct LaunchGrant {
    repository_id: u64,
    repository: String,
    server_url: &'static str,
    default_branch: String,
    event: String,
    event_ref: String,
    tested_sha: String,
    run_id: u64,
    run_attempt: u32,
    pull_request: Option<GrantedPull>,
    workflow_closure: Vec<VerifiedWorkflowIdentity>,
}

impl LaunchGrant {
    pub(super) fn describe(&self) -> String {
        let pull = self.pull_request.as_ref().map_or_else(String::new, |pull| {
            format!(
                " pull_request={} base={} head_repository={}({})",
                pull.number, pull.base_ref, pull.head_repository, pull.head_repository_id
            )
        });
        let workflows = self
            .workflow_closure
            .iter()
            .map(|identity| format!("{}={}", identity.path(), identity.content_sha256()))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "repository={}({}) server={} default_branch={} event={} ref={} sha={} run={}/{}{pull} workflows={workflows}",
            self.repository,
            self.repository_id,
            self.server_url,
            self.default_branch,
            self.event,
            self.event_ref,
            self.tested_sha,
            self.run_id,
            self.run_attempt,
        )
    }
}

pub(super) fn check_offer(
    connection: &RepositoryConnection,
    policy: Option<&ApprovedJobAuthorityPolicy>,
    token: &str,
    source: &SourceMetadata,
    deadline: Instant,
) -> Result<LaunchGrant, &'static str> {
    let policy = policy.ok_or("closed_world_unproven")?;
    let mut transport =
        HttpsTransport::with_deadline(GITHUB_API, deadline).map_err(HostError::code)?;
    let verified = authorize_source(connection, source, token, &mut transport)
        .map_err(crate::launch::steps::source::SourceBlock::code)?;
    verify_launch_source(connection, policy, &verified, token, deadline).map_err(HostError::code)
}

pub(super) fn verify_launch_source(
    connection: &RepositoryConnection,
    policy: &ApprovedJobAuthorityPolicy,
    verified: &VerifiedWorkflowSource,
    token: &str,
    deadline: Instant,
) -> Result<LaunchGrant, HostError> {
    let (owner, name) = connection
        .repository
        .split_once('/')
        .ok_or(HostError::Identity)?;
    let mut transport = HttpsTransport::with_deadline(GITHUB_API, deadline)?;
    let run = get_workflow_run_attempt(
        &mut transport,
        owner,
        name,
        verified.workflow_run_id,
        verified.run_attempt,
        token,
    )
    .map_err(|_| HostError::Identity)?;
    let (claim, pull) = if verified.event_name == "pull_request" {
        pull_claim(&mut transport, connection, verified, &run, token)?
    } else {
        (branch_claim(connection, verified, &run)?, None)
    };
    let checked = verify_source_claim(connection, policy, &claim, token, deadline)?;
    let default_branch = run
        .repository
        .default_branch
        .clone()
        .filter(|branch| !branch.is_empty())
        .ok_or(HostError::Identity)?;
    Ok(LaunchGrant {
        repository_id: claim.repository_id,
        repository: claim.repository_name,
        server_url: GITHUB_SERVER_URL,
        default_branch,
        event: claim.event_name,
        event_ref: claim.event_ref,
        tested_sha: claim.tested_sha,
        run_id: claim.run_id,
        run_attempt: claim.run_attempt,
        pull_request: pull,
        workflow_closure: checked.closure.identities,
    })
}

fn base_claim(
    connection: &RepositoryConnection,
    verified: &VerifiedWorkflowSource,
    run: &WorkflowRun,
    event_ref: String,
    tested_sha: String,
) -> Result<SourceClaim, HostError> {
    Ok(SourceClaim {
        repository_id: u64::try_from(connection.repository_id).map_err(|_| HostError::Identity)?,
        repository_name: connection.repository.clone(),
        run_id: u64::try_from(verified.workflow_run_id).map_err(|_| HostError::Identity)?,
        run_attempt: u32::try_from(verified.run_attempt).map_err(|_| HostError::Identity)?,
        event_name: verified.event_name.clone(),
        workflow_ref: format!(
            "{}/{}@{event_ref}",
            connection.repository,
            root_workflow_path(&run.path, &connection.repository)?
        ),
        event_ref,
        workflow_sha: tested_sha.clone(),
        tested_sha,
        job_workflow_ref: verified.job_workflow_ref.clone(),
        workflow_source_sha: verified.workflow_source_sha.clone(),
        pull_request: None,
    })
}

fn branch_claim(
    connection: &RepositoryConnection,
    verified: &VerifiedWorkflowSource,
    run: &WorkflowRun,
) -> Result<SourceClaim, HostError> {
    let reference = verified.job_workflow.reference.as_str();
    if !(reference.starts_with("refs/heads/") || reference.starts_with("refs/tags/")) {
        return Err(HostError::Identity);
    }
    base_claim(
        connection,
        verified,
        run,
        reference.to_owned(),
        run.head_sha.clone(),
    )
}

fn pull_claim(
    transport: &mut HttpsTransport,
    connection: &RepositoryConnection,
    verified: &VerifiedWorkflowSource,
    run: &WorkflowRun,
    token: &str,
) -> Result<(SourceClaim, Option<GrantedPull>), HostError> {
    let reference = verified.job_workflow.reference.as_str();
    let number = reference
        .strip_prefix("refs/pull/")
        .and_then(|value| value.strip_suffix("/merge"))
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or(HostError::Identity)?;
    let (owner, name) = connection
        .repository
        .split_once('/')
        .ok_or(HostError::Identity)?;
    let pull =
        get_pull_request(transport, owner, name, number, token).map_err(|_| HostError::Identity)?;
    let tested_sha = pull.merge_commit_sha.clone().ok_or(HostError::Identity)?;
    let head_repository = pull.head.repository.as_ref().ok_or(HostError::Identity)?;
    let number = u64::try_from(number).map_err(|_| HostError::Identity)?;
    let mut claim = base_claim(connection, verified, run, reference.to_owned(), tested_sha)?;
    claim.pull_request = Some(PullClaim {
        number,
        base_ref: pull.base.reference.clone(),
        base_sha: pull.base.sha.clone(),
        head_ref: pull.head.reference.clone(),
        head_sha: pull.head.sha.clone(),
    });
    let granted = GrantedPull {
        number,
        base_ref: pull.base.reference.clone(),
        head_repository_id: head_repository.id,
        head_repository: head_repository.full_name.clone(),
    };
    Ok((claim, Some(granted)))
}

fn root_workflow_path(path: &str, repository: &str) -> Result<String, HostError> {
    let path = path.split_once('@').map_or(path, |(path, _)| path);
    let path = path.strip_prefix("./").unwrap_or(path);
    let path = path.strip_prefix(&format!("{repository}/")).unwrap_or(path);
    if path.starts_with(".github/workflows/") {
        Ok(path.to_owned())
    } else {
        Err(HostError::Identity)
    }
}
