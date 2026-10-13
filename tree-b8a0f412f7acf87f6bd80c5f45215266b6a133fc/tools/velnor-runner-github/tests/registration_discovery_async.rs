//! Async repository bootstrap and cancellation behavior.
#![expect(
    clippy::expect_used,
    reason = "fixture setup uses checked expectations so failures identify their broken invariant"
)]

use std::{
    collections::VecDeque,
    future::{Future, pending},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

use velnor_runner_github::policy::{
    PoolAdmissionEvidence, PoolBindingView, PoolRegistrationScopeView, RunnerImageIdentityView,
    preflight_organization_pool_admission_async,
};
use velnor_runner_github::{
    ActionsServiceRouteLookup, AsyncDiscoveryIntentStore, AsyncDiscoveryTransport,
    AsyncScopedDiscoveryIntentStore, DiscoveryCredentialOutcome, DiscoveryCredentialStep,
    DiscoveryExchange, DiscoveryIntentId, DiscoveryStoreFuture, Exchange, RegistrationScope,
    SessionError, SessionRequest, TransportFail, WireError,
    exchange_organization_discovery_admin_once_async,
    exchange_repository_discovery_admin_once_async, issue_organization_discovery_token_async,
    issue_repository_discovery_token_async, organization_admin_evidence,
    read_repository_admin_evidence_async,
};

#[derive(Clone, Default)]
struct IntentState {
    rows: Vec<(
        DiscoveryIntentId,
        DiscoveryCredentialStep,
        Option<DiscoveryCredentialOutcome>,
    )>,
    events: Vec<&'static str>,
    fail_persist: bool,
    fail_finish: bool,
    scopes: Vec<String>,
}

#[derive(Clone)]
struct Intents(Arc<Mutex<IntentState>>, Option<Arc<AtomicUsize>>);

impl Default for Intents {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(IntentState::default())), None)
    }
}

impl AsyncDiscoveryIntentStore for Intents {
    fn persist_before<'a>(
        &'a mut self,
        step: DiscoveryCredentialStep,
        _repository_id: i64,
        _full_name: &'a str,
    ) -> DiscoveryStoreFuture<'a, DiscoveryIntentId> {
        let state = Arc::clone(&self.0);
        let intent_count = self.1.clone();
        Box::pin(async move {
            let mut state = state.lock().expect("test intent lock");
            state.events.push("intent");
            if state.fail_persist {
                return Err(WireError::Forbidden.into());
            }
            let id = DiscoveryIntentId::new(state.rows.len() as u64 + 1).expect("positive test id");
            state.rows.push((id, step, None));
            if let Some(count) = intent_count {
                count.fetch_add(1, Ordering::Release);
            }
            Ok(id)
        })
    }

    fn record_outcome(
        &mut self,
        id: DiscoveryIntentId,
        outcome: DiscoveryCredentialOutcome,
    ) -> DiscoveryStoreFuture<'_, ()> {
        let state = Arc::clone(&self.0);
        Box::pin(async move {
            let mut state = state.lock().expect("test intent lock");
            state.events.push("outcome");
            if state.fail_finish {
                return Err(WireError::Forbidden.into());
            }
            let row = state
                .rows
                .iter_mut()
                .find(|(row_id, _, _)| *row_id == id)
                .ok_or(WireError::Malformed)?;
            row.2 = Some(outcome);
            Ok(())
        })
    }
}

impl AsyncScopedDiscoveryIntentStore for Intents {
    fn persist_scope_before<'a>(
        &'a mut self,
        step: DiscoveryCredentialStep,
        scope: RegistrationScope<'a>,
        repository_id: i64,
        full_name: &'a str,
    ) -> DiscoveryStoreFuture<'a, DiscoveryIntentId> {
        let scope = match scope {
            RegistrationScope::Repository { owner, repo } => format!("repository:{owner}/{repo}"),
            RegistrationScope::Organization { org } => format!("organization:{org}"),
            RegistrationScope::Enterprise { enterprise } => format!("enterprise:{enterprise}"),
        };
        let state = Arc::clone(&self.0);
        let intent_count = self.1.clone();
        Box::pin(async move {
            let mut state = state.lock().expect("test intent lock");
            state.events.push("intent");
            if state.fail_persist || repository_id <= 0 || full_name.is_empty() {
                return Err(WireError::Forbidden.into());
            }
            let id = DiscoveryIntentId::new(state.rows.len() as u64 + 1).expect("positive test id");
            state.rows.push((id, step, None));
            state.scopes.push(scope);
            if let Some(count) = intent_count {
                count.fetch_add(1, Ordering::Release);
            }
            Ok(id)
        })
    }
}

#[derive(Default)]
struct TransportState {
    responses: VecDeque<Result<Exchange, TransportFail>>,
    requests: Vec<SessionRequest>,
    events: Vec<&'static str>,
    cancellations: Vec<Arc<AtomicBool>>,
    pending_next: bool,
    posts_sent: usize,
}

#[derive(Clone)]
struct FakeTransport(Arc<Mutex<TransportState>>, Option<Arc<AtomicUsize>>);

impl Default for FakeTransport {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(TransportState::default())), None)
    }
}

fn paired() -> (FakeTransport, Intents) {
    let intent_count = Arc::new(AtomicUsize::new(0));
    (
        FakeTransport(
            Arc::new(Mutex::new(TransportState::default())),
            Some(Arc::clone(&intent_count)),
        ),
        Intents(
            Arc::new(Mutex::new(IntentState::default())),
            Some(intent_count),
        ),
    )
}

impl AsyncDiscoveryTransport for FakeTransport {
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
        self.0
            .lock()
            .expect("test transport lock")
            .events
            .push("bind-api");
        Ok(())
    }

    fn bind_actions_service_origin(&mut self, _url: &str) -> Result<(), SessionError> {
        self.0
            .lock()
            .expect("test transport lock")
            .events
            .push("bind-actions");
        Ok(())
    }

    fn exchange_discovery(&mut self, request: SessionRequest) -> DiscoveryExchange {
        let (response, cancellation, pending_next) = {
            let mut state = self.0.lock().expect("test transport lock");
            if request.method == velnor_runner_github::Method::Post {
                if let Some(intent_count) = &self.1 {
                    assert!(
                        intent_count.load(Ordering::Acquire) > state.posts_sent,
                        "every discovery POST must have a prior durable intent"
                    );
                }
                state.posts_sent += 1;
            }
            state.events.push("send");
            state.requests.push(request);
            let cancellation = Arc::new(AtomicBool::new(false));
            state.cancellations.push(Arc::clone(&cancellation));
            (
                state.responses.pop_front(),
                cancellation,
                state.pending_next,
            )
        };
        if pending_next {
            DiscoveryExchange::new(pending(), cancellation)
        } else {
            DiscoveryExchange::new(
                async move { response.unwrap_or(Err(TransportFail::Reset)) },
                cancellation,
            )
        }
    }
}

#[expect(
    clippy::panic,
    reason = "a pending single-poll fake future is a test failure"
)]
fn block_on_ready<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("test future unexpectedly pending"),
    }
}

fn poll_once<F: Future>(future: &mut Pin<Box<F>>) -> Poll<F::Output> {
    let mut context = Context::from_waker(Waker::noop());
    future.as_mut().poll(&mut context)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the scripted transport queue stores exchange-or-failure results"
)]
fn response(status: u16, body: &str) -> Result<Exchange, TransportFail> {
    Ok(Exchange {
        status,
        body: body.as_bytes().to_vec(),
    })
}

fn private_admin_repo() -> &'static str {
    r#"{"id":829618808,"full_name":"ChainArgos/java-monorepo","private":true,"permissions":{"admin":true}}"#
}

#[path = "registration_discovery_async/bootstrap.rs"]
mod bootstrap;
#[path = "registration_discovery_async/lifecycle.rs"]
mod lifecycle;
#[path = "registration_discovery_async/preflight.rs"]
mod preflight;
#[path = "registration_discovery_async/reconciliation.rs"]
mod reconciliation;
#[path = "registration_discovery_async/routes.rs"]
mod routes;
