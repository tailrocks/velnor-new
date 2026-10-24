use std::cell::{Cell, RefCell};

use velnor_runner_github::{
    AckScope, Exchange, Method, Poll, QueueSession, RefreshGate, SessionRequest, Transport,
    TransportFail, WireError,
};

use crate::launch::test_positive_poll_once_with_response;

use super::*;

#[path = "ack_uncertain/setup.rs"]
mod setup;

struct PollFixture {
    scratch: crate::launch::turn::scoped_poll_tests::support::Scratch,
    scope: crate::journal::ConnectionJournal,
    intent_id: i64,
    allocation: crate::journal::HeaderAllocation,
    session: QueueSession,
    link: crate::listen::Link,
}

struct SessionResponse;

impl Transport for SessionResponse {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Post);
        Ok(Exchange {
            status: 200,
            body: br#"{"sessionId":"session-scoped-poll","messageQueueUrl":"https://queue.example.invalid/","messageQueueAccessToken":"queue-token"}"#.to_vec(),
        })
    }
}

struct AckResponse {
    status: u16,
    requests: Vec<SessionRequest>,
}

impl Transport for AckResponse {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push(request.clone());
        Ok(Exchange {
            status: self.status,
            body: Vec::new(),
        })
    }
}

async fn poll_fixture() -> Result<PollFixture, String> {
    use crate::launch::turn::scoped_poll_tests::support::Scratch;

    let scratch = Scratch::new()?;
    let (scope, intent_id, allocation) = setup::active_scope(&scratch).await?;
    let mut response = SessionResponse;
    let session =
        velnor_runner_github::create_session(&mut response, 10_101, "owner", "admin-token")
            .map_err(|error| error.to_string())?;
    let link = crate::listen::inert_link_for_offline_test();
    Ok(PollFixture {
        scratch,
        scope,
        intent_id,
        allocation,
        session,
        link,
    })
}

fn available_poll() -> Poll {
    crate::launch_harness::available(&[91])
}

async fn authorized_offer(
    scope: &crate::journal::ConnectionJournal,
    connection: &crate::config::RepositoryConnection,
    polled: &Poll,
    expected_result: crate::journal::SourceAuthorizationResult,
) -> Result<crate::launch::DeferredAuthorizedOffer, String> {
    use crate::config::SourcePolicyMode;
    use crate::journal::AssignmentKey;
    use crate::launch::turn::scoped_poll_tests::support::authorized_source;
    use crate::launch::{DeferredAuthorizedOffer, steps::source::test_receipt};

    let Poll::Batch(batch) = polled else {
        return Err("The test response has no batch.".to_owned());
    };
    let request_id = 91;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await
        .map_err(|error| error.to_string())?;
    let binding = scope
        .source_authorization_binding()
        .await
        .map_err(|error| error.to_string())?;
    let receipt = test_receipt(
        &binding,
        request_id,
        batch.message_id,
        authorized_source(),
        SourcePolicyMode::Repository,
    );
    let result = scope
        .record_source_authorization(&identity, &receipt)
        .await
        .map_err(|error| error.to_string())?;
    if result != expected_result {
        return Err("The request source result did not match the expected result.".to_owned());
    }
    DeferredAuthorizedOffer::from_poll_receipt(
        connection, scope, 10_101, polled, request_id, receipt,
    )
    .await
    .map_err(|error| error.to_string())
}

fn assert_uncertain_outcome(
    outcome: crate::launch::PositivePollOutcome,
    ack_calls: &Cell<usize>,
    admission_calls: &Cell<usize>,
    ack_response: &RefCell<AckResponse>,
    message_id: i64,
) -> Result<crate::launch::DeferredAuthorizedOffer, String> {
    assert_eq!(ack_calls.get(), 1);
    assert_eq!(admission_calls.get(), 1);
    assert_eq!(outcome.acknowledgement, Acknowledgement::Uncertain);
    assert_eq!(outcome.settlement, Settlement::Settled);
    assert_eq!(
        outcome.cursor_disposition,
        CursorDisposition::Deferred { message_id }
    );
    assert_eq!(ack_response.borrow().requests.len(), 1);
    assert_eq!(ack_response.borrow().requests[0].method, Method::Delete);
    assert_eq!(
        ack_response.borrow().requests[0].path,
        format!("messages/{message_id}")
    );
    let Some(PositiveAdmission::Deferred(offer)) = outcome.admission else {
        return Err(
            "The poll outcome did not retain the authorized request after an uncertain ACK."
                .to_owned(),
        );
    };
    Ok(offer)
}

async fn assert_offer_replays(
    scratch: &crate::launch::turn::scoped_poll_tests::support::Scratch,
    connection: &crate::config::RepositoryConnection,
    polled: &Poll,
    retained: &crate::launch::DeferredAuthorizedOffer,
) -> Result<(), String> {
    let journal = crate::journal::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let reopened = journal
        .connection_scope(connection)
        .await
        .map_err(|error| error.to_string())?;
    let replay = authorized_offer(
        &reopened,
        connection,
        polled,
        crate::journal::SourceAuthorizationResult::Replayed,
    )
    .await?;
    assert!(authorized_deferred_matches(&replay, polled, &reopened, 10_101).await);
    assert_same_authorization(replay.offer(), retained.offer());
    Ok(())
}

fn assert_same_authorization(
    replay: &crate::journal::AuthorizedOffer,
    retained: &crate::journal::AuthorizedOffer,
) {
    assert_eq!(replay.connection_id(), retained.connection_id());
    assert_eq!(replay.scale_set_id(), retained.scale_set_id());
    assert_eq!(replay.request_id(), retained.request_id());
    assert_eq!(replay.message_id(), retained.message_id());
    assert_eq!(replay.binding_digest(), retained.binding_digest());
    assert_eq!(replay.source_digest(), retained.source_digest());
    assert_eq!(replay.source_json(), retained.source_json());
    assert_eq!(replay.policy_mode(), retained.policy_mode());
}

#[tokio::test]
async fn http_401_ack_keeps_offer_durable_for_exact_replay() -> Result<(), String> {
    let mut fixture = poll_fixture().await?;
    let connection = crate::launch::turn::scoped_poll_tests::support::repository_connection()?;
    let polled = available_poll();
    let Poll::Batch(batch) = &polled else {
        return Err("The test response has no batch.".to_owned());
    };
    let message_id = batch.message_id;
    let offer = authorized_offer(
        &fixture.scope,
        &connection,
        &polled,
        crate::journal::SourceAuthorizationResult::Recorded,
    )
    .await?;
    let ack_response = RefCell::new(AckResponse {
        status: 401,
        requests: Vec::new(),
    });
    let ack_calls = Cell::new(0);
    let admission_calls = Cell::new(0);
    let mut pending_offer = Some(offer);
    let mut link = fixture.link;
    let outcome = test_positive_poll_once_with_response(
        crate::launch::PositivePollContext {
            link: &mut link,
            set_id: 10_101,
            session: &mut fixture.session,
            scoped: &fixture.scope,
            session_intent_id: fixture.intent_id,
            allocation: &fixture.allocation,
            cursor: 0,
        },
        crate::launch::TestPollResponse {
            polled: polled.clone(),
            path: "messages".to_owned(),
            queue: None,
        },
        async |_link, session, path, _queue, response| {
            ack_calls.set(ack_calls.get() + 1);
            let Poll::Batch(batch) = response else {
                return Err(crate::scale_set::EnsureError::Malformed);
            };
            let scope = AckScope {
                replay_safe: true,
                sole_unacquired_offer: false,
                queue_token: session.token(),
            };
            velnor_runner_github::ack(
                &mut *ack_response.borrow_mut(),
                &path,
                batch,
                &scope,
                &RefreshGate::new(),
                || Err(WireError::RefreshExhausted),
            )
            .map(|_| ())
            .map_err(crate::listen::map_listen)
        },
        async |_link, _response, _queue, _path, _queue_token, _admin_token| {
            admission_calls.set(admission_calls.get() + 1);
            pending_offer
                .take()
                .map(PositiveAdmission::Deferred)
                .ok_or(crate::scale_set::EnsureError::Uncertain)
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    let retained = assert_uncertain_outcome(
        outcome,
        &ack_calls,
        &admission_calls,
        &ack_response,
        message_id,
    )?;
    assert_eq!(retained.offer().request_id(), 91);
    assert_eq!(retained.offer().message_id(), message_id);
    drop(fixture.scope);
    assert_offer_replays(&fixture.scratch, &connection, &polled, &retained).await
}

#[tokio::test]
async fn two_available_request_ids_stop_before_acquisition() -> Result<(), String> {
    let mut fixture = poll_fixture().await?;
    let connection = crate::launch::turn::scoped_poll_tests::support::repository_connection()?;
    let polled = crate::launch_harness::available(&[91, 92]);
    let admission_calls = Cell::new(0);
    let acquire_calls = Cell::new(0);
    let ack_calls = Cell::new(0);
    let mut link = fixture.link;
    let result = test_positive_poll_once_with_response(
        crate::launch::PositivePollContext {
            link: &mut link,
            set_id: 10_101,
            session: &mut fixture.session,
            scoped: &fixture.scope,
            session_intent_id: fixture.intent_id,
            allocation: &fixture.allocation,
            cursor: 0,
        },
        crate::launch::TestPollResponse {
            polled: polled.clone(),
            path: "messages".to_owned(),
            queue: None,
        },
        async |_link, _session, _path, _queue, _response| {
            ack_calls.set(ack_calls.get() + 1);
            Ok(())
        },
        async |_link, response, _queue, _path, _queue_token, _admin_token| {
            admission_calls.set(admission_calls.get() + 1);
            crate::launch::authorize_positive_offer(
                &connection,
                &fixture.scope,
                "test-token",
                10_101,
                response,
            )
            .await?;
            acquire_calls.set(acquire_calls.get() + 1);
            Ok(PositiveAdmission::NoAcquire)
        },
    )
    .await;

    let Err(error) = result else {
        return Err("The poll accepted more than one available request.".to_owned());
    };
    assert_eq!(
        error,
        crate::scale_set::EnsureError::Unexpected {
            status: 0,
            step: "capacity",
        }
    );
    assert_eq!(admission_calls.get(), 1);
    assert_eq!(acquire_calls.get(), 0);
    assert_eq!(ack_calls.get(), 0);
    for request_id in [91, 92] {
        let identity = fixture
            .scope
            .assignment_identity(crate::journal::AssignmentKey::AssignedRequest { request_id })
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(
            fixture
                .scope
                .request_acquire_state(&identity)
                .await
                .map_err(|error| error.to_string())?,
            None
        );
    }
    Ok(())
}
