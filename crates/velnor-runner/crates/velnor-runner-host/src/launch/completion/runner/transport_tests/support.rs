use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_runner_github::TransportFail;
use velnor_runner_github::session::{Exchange, Method, SessionRequest, Transport};

use super::super::super::cleanup_retry_delay_seconds;
use crate::error::HostError;
use crate::journal::request_lifecycle::tests::{
    NativeStartedIntakeFixture, native_started_intake_fixture,
};
use crate::journal::{CleanupClaim, CompletedLaunch, ConnectionJournal};
use crate::launch::completion::journal_ref::CompletionJournal;
use crate::listen::Secret;

const EFFECT_LEASE_SECONDS: i64 = 90;

pub(super) fn runner_body(id: i64, name: &str, scale_set_id: i64) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "count": 1,
        "value": [{ "id": id, "name": name, "runnerScaleSetId": scale_set_id }]
    }))
    .expect("runner fixture serializes")
}

pub(super) fn empty_runners() -> Vec<u8> {
    br#"{"count":0,"value":[]}"#.to_vec()
}

pub(super) enum Response {
    Json(u16, Vec<u8>),
    Status(u16),
    Failure(TransportFail),
}

impl Response {
    pub(super) fn json(status: u16, body: Vec<u8>) -> Self {
        Self::Json(status, body)
    }

    pub(super) fn status(status: u16) -> Self {
        Self::Status(status)
    }
}

pub(super) struct ScriptedTransport {
    pub(super) responses: VecDeque<Response>,
    pub(super) requests: Vec<CapturedRequest>,
}

impl ScriptedTransport {
    pub(super) fn new(responses: impl IntoIterator<Item = Response>) -> Self {
        Self {
            responses: responses.into_iter().collect(),
            requests: Vec::new(),
        }
    }
}

impl Transport for ScriptedTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push(CapturedRequest {
            method: request.method,
            path: request.path.clone(),
            query: request.query.clone(),
        });
        match self.responses.pop_front().ok_or(TransportFail::Reset)? {
            Response::Json(status, body) => Ok(Exchange { status, body }),
            Response::Status(status) => Ok(Exchange {
                status,
                body: Vec::new(),
            }),
            Response::Failure(error) => Err(error),
        }
    }
}

pub(super) struct CapturedRequest {
    pub(super) method: Method,
    pub(super) path: String,
    pub(super) query: Option<String>,
}

pub(super) struct Fixture {
    pub(super) scope: ConnectionJournal,
    pub(super) completion: CompletionJournal,
    intake: NativeStartedIntakeFixture,
    pub(super) admin: Secret,
}

impl Fixture {
    pub(super) async fn new(label: &str) -> Result<Self, HostError> {
        let _ = label;
        let intake = native_started_intake_fixture().await?;
        let scope = intake.scope.clone();
        let admin = Secret::new("test token");
        Ok(Self {
            scope,
            completion: CompletionJournal::scoped(intake.scope.clone()),
            intake,
            admin,
        })
    }

    pub(super) async fn claim(&self) -> Result<(CompletedLaunch, CleanupClaim), HostError> {
        self.intake.record_started().await?;
        let completed = self
            .scope
            .record_job_completed(
                &self.intake.assignment,
                self.intake.message_id,
                &self.intake.source,
                Some("success"),
                Some(&self.intake.runner),
                Some("opaque-started-job"),
            )
            .await?;
        if completed != Some(self.intake.intent_id) {
            return Err(HostError::Journal);
        }
        let launch = self.due(self.intake.intent_id).await?;
        let claim = self
            .scope
            .claim_completion_cleanup(launch.intent.id, EFFECT_LEASE_SECONDS)
            .await?
            .ok_or(HostError::Journal)?;
        Ok((launch, claim))
    }

    pub(super) async fn due(&self, id: i64) -> Result<CompletedLaunch, HostError> {
        self.scope
            .due_completed_launches(
                unix_seconds()?.checked_add(200).ok_or(HostError::Journal)?,
                10,
            )
            .await?
            .into_iter()
            .find(|launch| launch.intent.id == id)
            .ok_or(HostError::Journal)
    }

    pub(super) async fn retry_claim(
        &self,
        id: i64,
        previous: CleanupClaim,
    ) -> Result<CleanupClaim, HostError> {
        let now = unix_seconds()?;
        let retry_after = now
            .checked_add(cleanup_retry_delay_seconds(previous.attempt))
            .ok_or(HostError::Journal)?;
        if !self
            .scope
            .retry_completion_cleanup_at(id, previous.generation, now, retry_after)
            .await?
        {
            return Err(HostError::Journal);
        }
        self.scope
            .claim_completion_cleanup_at(id, retry_after, EFFECT_LEASE_SECONDS)
            .await?
            .ok_or(HostError::Journal)
    }
}

pub(super) fn assert_request(
    request: &CapturedRequest,
    method: Method,
    path: &str,
    query: Option<&str>,
) {
    assert_eq!(request.method, method);
    assert_eq!(request.path, path);
    assert_eq!(request.query.as_deref(), query);
}

pub(super) fn assert_runner_lookup(request: &CapturedRequest, method: Method, name: &str) {
    let query = format!("agentName={name}&api-version=6.0-preview");
    assert_request(
        request,
        method,
        "_apis/distributedtask/pools/0/agents",
        Some(&query),
    );
}

fn unix_seconds() -> Result<i64, HostError> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_secs(),
    )
    .map_err(|_| HostError::Journal)
}
