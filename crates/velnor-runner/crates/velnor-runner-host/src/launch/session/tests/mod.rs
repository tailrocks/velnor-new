use std::collections::VecDeque;
use std::path::PathBuf;

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::connect::SetOwnership;
use crate::journal::{
    CapacityLimits, ConnectionJournal, HeaderAllocation, Journal, SessionRefreshState,
};
use velnor_runner_github::{
    Exchange, Method, Poll, RefreshGate, SessionRequest, Transport, TransportFail, WireError, poll,
};

use super::delete::delete_removed_connection_session_with;
use super::refresh::refresh_bound_connection_session_with;
use super::{CompletedHeaderPoll, SessionDeleteOutcome};

#[path = "poll_refresh_tests.rs"]
mod poll_refresh_tests;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "velnor-header-zero-proof-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ignored = std::fs::remove_dir_all(&self.0);
    }
}

fn binding() -> Result<RepositoryConnection, String> {
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )
    .map_err(|error| error.to_string())?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    RepositoryConnection::try_new(
        ConnectionId::parse("header-zero-proof").map_err(|error| error.to_string())?,
        101,
        "owner/repository".to_owned(),
        KeychainReference::parse("keychain:velnor/header-zero-proof")
            .map_err(|error| error.to_string())?,
        selector,
        policy,
    )
    .map_err(|error| error.to_string())
}

struct DeleteTransport {
    status: u16,
    body: Vec<u8>,
    calls: Vec<(Method, String, Option<String>, Option<String>)>,
}

impl Transport for DeleteTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let authorization = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone());
        self.calls.push((
            request.method,
            request.path.clone(),
            request.query.clone(),
            authorization,
        ));
        Ok(Exchange {
            status: self.status,
            body: self.body.clone(),
        })
    }
}

struct RefreshTransport {
    responses: VecDeque<Result<Exchange, TransportFail>>,
    requests: Vec<SessionRequest>,
}

impl RefreshTransport {
    fn new(responses: impl IntoIterator<Item = Result<Exchange, TransportFail>>) -> Self {
        Self {
            responses: responses.into_iter().collect(),
            requests: Vec::new(),
        }
    }
}

impl Transport for RefreshTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push(request.clone());
        self.responses
            .pop_front()
            .unwrap_or(Err(TransportFail::Reset))
    }
}

fn refreshed_body(session_id: &str, queue_url: &str, token: &str) -> Vec<u8> {
    format!(
            r#"{{"sessionId":"{session_id}","messageQueueUrl":"{queue_url}","messageQueueAccessToken":"{token}"}}"#
        )
        .into_bytes()
}

async fn bound_session(
    scratch: &Scratch,
    target: u32,
    session_id: &str,
) -> Result<(Journal, ConnectionJournal, i64, HeaderAllocation), String> {
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: 2_000,
            memory_bytes: 2_147_483_648,
        })
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("test-engine")
        .await
        .map_err(|error| error.to_string())?;
    let binding = binding()?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| error.to_string())?;
    scope
        .bind_scale_set(10_101, "integration", SetOwnership::Adopted)
        .await
        .map_err(|error| error.to_string())?;
    let prepared = scope
        .prepare_session_create("fedcba9876543210fedcba9876543210")
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_session_create(prepared.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "session create claim missing".to_owned())?;
    let bound = scope
        .record_created_session(&claim, session_id, target)
        .await
        .map_err(|error| error.to_string())?;
    Ok((
        journal,
        scope,
        bound.intent_id(),
        bound.allocation().clone(),
    ))
}

#[path = "delete_cases.rs"]
mod delete_cases;
#[path = "refresh_cases.rs"]
mod refresh_cases;
