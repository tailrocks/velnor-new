// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use velnor_resource_probe::job_hook::{Challenge, Decision, sha256};

use crate::launch::execution_authorization::ExecutionSourceRequest;

use super::authority::{CallError, Client, RunAnswer};
use super::plan::Side;
use super::policy::Policy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Reason {
    RunUnknown,
    NotDispatched,
    WrongSide,
    AuthorityUnreachable,
    RequestMalformed,
    RequestMismatch,
    Policy,
}

impl Reason {
    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::RunUnknown => "run_unknown",
            Self::NotDispatched => "not_dispatched",
            Self::WrongSide => "wrong_side",
            Self::AuthorityUnreachable => "authority_unreachable",
            Self::RequestMalformed => "request_malformed",
            Self::RequestMismatch => "request_mismatch",
            Self::Policy => "policy",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Facts {
    pub(super) run_id: u64,
    pub(super) attempt: u32,
    pub(super) job: String,
    pub(super) event: String,
    pub(super) git_ref: String,
    pub(super) sha: String,
}

#[derive(Debug, Clone)]
pub(super) struct Verdict {
    pub(super) reason: Option<Reason>,
    pub(super) ledger: Option<String>,
    pub(super) facts: Option<Facts>,
    pub(super) decision: Decision,
}

pub(super) async fn decide(
    client: &Client,
    side: Side,
    policy: Option<&Policy>,
    challenge: &Challenge,
    bytes: &[u8],
) -> Verdict {
    let Ok(request) = ExecutionSourceRequest::parse(bytes) else {
        return refused(challenge, bytes, None, Reason::RequestMalformed);
    };
    let facts = Facts {
        run_id: request.run_id(),
        attempt: request.run_attempt(),
        job: request.job_key().to_owned(),
        event: request.event_name().to_owned(),
        git_ref: request.event_ref().to_owned(),
        sha: request.tested_sha().to_owned(),
    };
    let ids = request.challenge() == challenge.nonce
        && request.reservation_id() == challenge.reservation_id
        && request.intent_id() == challenge.intent_id;
    if !ids {
        return refused(challenge, bytes, Some(facts), Reason::RequestMismatch);
    }
    let answer = client.run(side, facts.run_id, facts.attempt).await;
    judge(challenge, bytes, &request, (side, policy), facts, answer)
}

pub(super) fn judge(
    challenge: &Challenge,
    bytes: &[u8],
    request: &ExecutionSourceRequest,
    (side, policy): (Side, Option<&Policy>),
    facts: Facts,
    answer: Result<Option<RunAnswer>, CallError>,
) -> Verdict {
    let answer = match answer {
        Err(_) => return refused(challenge, bytes, Some(facts), Reason::AuthorityUnreachable),
        Ok(None) => return refused(challenge, bytes, Some(facts), Reason::RunUnknown),
        Ok(Some(answer)) => answer,
    };
    if !describes(request, &answer) {
        return refused(challenge, bytes, Some(facts), Reason::RequestMismatch);
    }
    let Some(ledger) = answer.ledger else {
        let reason = match answer.refusal.as_deref() {
            Some("wrong_side") => Reason::WrongSide,
            _ => Reason::NotDispatched,
        };
        return refused(challenge, bytes, Some(facts), reason);
    };
    if policy
        .is_some_and(|policy| !policy.approves(request, side, answer.workflow_sha256.as_deref()))
    {
        return refused(challenge, bytes, Some(facts), Reason::Policy);
    }
    let Some(actual_request_id) = i64::try_from(facts.run_id).ok().filter(|id| *id > 0) else {
        return refused(challenge, bytes, Some(facts), Reason::RequestMalformed);
    };
    let decision = Decision {
        allow: true,
        actual_request_id,
        native_job_id: Some(format!("{}:{}:{}", facts.run_id, facts.attempt, facts.job)),
        ..template(challenge, bytes, Some(&facts))
    };
    Verdict {
        reason: None,
        ledger: Some(ledger),
        facts: Some(facts),
        decision,
    }
}

fn describes(request: &ExecutionSourceRequest, answer: &RunAnswer) -> bool {
    workflow_path(request.workflow_ref(), request.repository_name())
        .is_some_and(|path| path == answer.workflow)
        && request.run_id() == answer.id
        && request.run_attempt() == answer.attempt
        && request.event_name() == answer.event
        && request.event_ref() == answer.git_ref
        && request.tested_sha() == answer.sha
        && answer.jobs.iter().any(|job| job == request.job_key())
}

pub(super) fn workflow_path<'a>(workflow_ref: &'a str, repository: &str) -> Option<&'a str> {
    let path = workflow_ref.strip_prefix(repository)?.strip_prefix('/')?;
    path.rsplit_once('@').map(|(path, _)| path)
}

fn refused(challenge: &Challenge, bytes: &[u8], facts: Option<Facts>, reason: Reason) -> Verdict {
    let decision = Decision {
        deny_code: Some(reason.code().to_owned()),
        ..template(challenge, bytes, facts.as_ref())
    };
    Verdict {
        reason: Some(reason),
        ledger: None,
        facts,
        decision,
    }
}

fn template(challenge: &Challenge, bytes: &[u8], facts: Option<&Facts>) -> Decision {
    Decision {
        schema_version: 1,
        allow: false,
        challenge: challenge.nonce.clone(),
        request_sha256: sha256(bytes),
        reservation_id: challenge.reservation_id,
        intent_id: challenge.intent_id,
        actual_request_id: challenge.witness_request_id,
        run_id: facts.map_or(0, |facts| facts.run_id),
        run_attempt: facts.map_or(0, |facts| facts.attempt),
        job_key: facts.map_or_default(|facts| facts.job.clone()),
        runner_id: challenge.runner_id,
        policy_generation: challenge.policy_generation,
        policy_digest: challenge.policy_digest.clone(),
        native_job_id: None,
        rest_job_id: None,
        deny_code: None,
    }
}
