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

use velnor_runner_github::{
    AsyncDiscoveryIntentStore, AsyncDiscoveryTransport, DiscoveryCredentialOutcome,
    DiscoveryCredentialStep, DiscoveryExchange, DiscoveryIntentId, DiscoveryStoreFuture, Exchange,
    SessionError, SessionRequest, TransportFail, WireError,
    exchange_repository_discovery_admin_once_async, issue_repository_discovery_token_async,
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

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the end-to-end test verifies ordering across the full read-only bootstrap"
)]
fn async_discovery_persists_before_each_post_and_never_exposes_credentials() {
    let (mut transport, mut intents) = paired();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .extend([
            response(200, private_admin_repo()),
            response(201, r#"{"token":"regtoken"}"#),
            response(
                200,
                r#"{"url":"https://pipelinesghubeus9.actions.githubusercontent.com/","token":"admintoken"}"#,
            ),
            response(200, r#"{"count":1,"value":[{"id":1,"name":"Default","isDefaultGroup":true}]}"#),
            response(
                200,
                r#"{"count":1,"value":[{"id":3,"name":"ubuntu-24.04-scale-set","labels":[{"name":"velnor","type":"System"},{"name":"ubuntu-24.04-scale-set","type":"System"}],"runnerSetting":{"disableUpdate":true}}]}"#,
            ),
        ]);
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let token = block_on_ready(issue_repository_discovery_token_async(
        &mut transport,
        evidence,
        "hostcredential",
        &mut intents,
    ))
    .expect("one-shot registration token");
    let admin = block_on_ready(exchange_repository_discovery_admin_once_async(
        &mut transport,
        token,
        &mut intents,
    ))
    .expect("one-shot admin exchange");
    let groups = block_on_ready(admin.list_runner_groups_async(&mut transport))
        .expect("read-only group metadata");
    let scale_set = block_on_ready(admin.get_existing_product_scale_set_async(
        &mut transport,
        1,
        "ubuntu-24.04-scale-set",
    ))
    .expect("read-only exact scale set lookup");

    assert_eq!(groups.len(), 1);
    assert!(matches!(
        scale_set,
        velnor_runner_github::ScaleSetFound::Found(view) if view.id == 3
    ));
    let state = intents.0.lock().expect("test intent lock").clone();
    assert_eq!(
        state.rows,
        vec![
            (
                DiscoveryIntentId::new(1).expect("positive id"),
                DiscoveryCredentialStep::RepositoryRegistrationToken,
                Some(DiscoveryCredentialOutcome::Succeeded),
            ),
            (
                DiscoveryIntentId::new(2).expect("positive id"),
                DiscoveryCredentialStep::ActionsAdminExchange,
                Some(DiscoveryCredentialOutcome::Succeeded),
            ),
        ]
    );
    assert_eq!(state.events, ["intent", "outcome", "intent", "outcome"]);
    let transport_state = transport.0.lock().expect("test transport lock");
    let requests = &transport_state.requests;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].path, "repos/ChainArgos/java-monorepo");
    assert_eq!(
        requests[1].path,
        "/repos/ChainArgos/java-monorepo/actions/runners/registration-token"
    );
    assert_eq!(requests[2].path, "/actions/runner-registration");
    assert_eq!(requests[3].path, "_apis/runtime/runnergroups");
    assert_eq!(requests[4].path, "_apis/runtime/runnerscalesets");
    let debug = format!("{admin:?} {:?}", requests[2]);
    assert!(!debug.contains("admintoken"));
    assert!(!debug.contains("regtoken"));
}

#[path = "registration_discovery_async/lifecycle.rs"]
mod lifecycle;
