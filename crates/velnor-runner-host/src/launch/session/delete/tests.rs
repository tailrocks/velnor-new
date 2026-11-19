// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{
    Exchange, Method, QueueSession, SessionRequest, Transport, TransportFail,
};

use super::{delete_removed_connection_session_with, retire_session_of_deleted_set_with};
use crate::journal::{CapacityLimits, OperationalCapacityLimits};
use crate::launch::{CompletedHeaderPoll, SessionDeleteOutcome};
use crate::scale_set::delete::tests::{Fixture, fixture};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const NONCE: &str = "0123456789abcdef0123456789abcdef";

struct Answer {
    status: u16,
    seen: Vec<(Method, String)>,
}

impl Answer {
    const fn new(status: u16) -> Self {
        Self {
            status,
            seen: Vec::new(),
        }
    }
}

impl Transport for Answer {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push((request.method, request.path.clone()));
        Ok(Exchange {
            status: self.status,
            body: Vec::new(),
        })
    }
}

fn queue_session() -> TestResult<QueueSession> {
    struct Queue;
    impl Transport for Queue {
        fn exchange(&mut self, _: &SessionRequest) -> Result<Exchange, TransportFail> {
            Ok(Exchange {
                status: 200,
                body: br#"{"sessionId":"session-1","messageQueueUrl":"https://queue.example.com/messages","messageQueueAccessToken":"queue-token"}"#.to_vec(),
            })
        }
    }
    Ok(velnor_runner_github::create_session(
        &mut Queue, 10, "owner", "admin",
    )?)
}

async fn ready_to_delete(fixture: &Fixture) -> TestResult<(i64, crate::journal::HeaderAllocation)> {
    fixture
        .journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 2,
                cpu_millicores: 6_000,
                memory_bytes: 13 << 30,
            },
            OperationalCapacityLimits {
                cpu_millicores: 500,
                memory_bytes: 1 << 30,
            },
        )
        .await?;
    let scoped = &fixture.scoped;
    scoped.prepare_session_create(NONCE).await?;
    let claim = scoped
        .claim_session_create(NONCE)
        .await?
        .ok_or("session claim")?;
    let intent = scoped
        .record_created_session(&claim, "session-1", 1)
        .await?
        .intent_id();
    let allocation = scoped.session_header_allocation(intent).await?;
    let poll = scoped.begin_zero_header_poll(intent, &allocation).await?;
    scoped
        .settle_session_poll(intent, CompletedHeaderPoll::for_test(poll))
        .await?;
    Ok((intent, allocation))
}

async fn record_set_delete(fixture: &Fixture, state: &str) -> TestResult {
    let connection = fixture.journal.connection().await?;
    connection
        .execute(
            "INSERT INTO connection_scale_set_deletes (connection_id, scale_set_id, runner_group_id, state) VALUES ('conn-test-1', 10, 1, ?1)",
            [state],
        )
        .await?;
    Ok(())
}

async fn delete_with(
    fixture: &Fixture,
    status: u16,
    intent: i64,
    allocation: &crate::journal::HeaderAllocation,
) -> TestResult<(SessionDeleteOutcome, Answer)> {
    let mut transport = Answer::new(status);
    let outcome = delete_removed_connection_session_with(
        &mut transport,
        "admin-token",
        &queue_session()?,
        &fixture.scoped,
        intent,
        allocation,
    )
    .await;
    Ok((outcome, transport))
}

#[tokio::test]
async fn a_session_in_a_set_we_deleted_is_gone_and_the_connection_retires() -> TestResult {
    let fixture = fixture(true).await?;
    let (intent, allocation) = ready_to_delete(&fixture).await?;
    record_set_delete(&fixture, "deleted").await?;
    let (outcome, transport) = delete_with(&fixture, 404, intent, &allocation).await?;
    assert_eq!(outcome, SessionDeleteOutcome::Deleted);
    assert_eq!(transport.seen.len(), 1);
    assert!(
        transport
            .seen
            .iter()
            .all(|(method, path)| *method == Method::Delete
                && path.ends_with("/10/sessions/session-1")),
        "{:?}",
        transport.seen
    );
    assert!(
        fixture
            .journal
            .retire_connection_if_drained(&fixture.binding)
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn a_404_for_a_set_we_did_not_delete_stays_uncertain() -> TestResult {
    for state in [None, Some("requested"), Some("uncertain"), Some("prepared")] {
        let fixture = fixture(true).await?;
        let (intent, allocation) = ready_to_delete(&fixture).await?;
        if let Some(state) = state {
            record_set_delete(&fixture, state).await?;
        }
        let (outcome, _) = delete_with(&fixture, 404, intent, &allocation).await?;
        assert_eq!(outcome, SessionDeleteOutcome::Held, "{state:?}");
        assert!(
            !fixture
                .journal
                .retire_connection_if_drained(&fixture.binding)
                .await?,
            "{state:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_204_still_closes_the_session_and_the_connection_retires() -> TestResult {
    let fixture = fixture(true).await?;
    let (intent, allocation) = ready_to_delete(&fixture).await?;
    let (outcome, _) = delete_with(&fixture, 204, intent, &allocation).await?;
    assert_eq!(outcome, SessionDeleteOutcome::Deleted);
    assert!(
        fixture
            .journal
            .retire_connection_if_drained(&fixture.binding)
            .await?
    );
    Ok(())
}

async fn positive_header_session(fixture: &Fixture, uncertain_poll: bool) -> TestResult<i64> {
    fixture
        .journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 2,
                cpu_millicores: 6_000,
                memory_bytes: 13 << 30,
            },
            OperationalCapacityLimits {
                cpu_millicores: 500,
                memory_bytes: 1 << 30,
            },
        )
        .await?;
    let scoped = &fixture.scoped;
    scoped.prepare_session_create(NONCE).await?;
    let claim = scoped
        .claim_session_create(NONCE)
        .await?
        .ok_or("session claim")?;
    let intent = scoped
        .record_created_session(&claim, "session-1", 1)
        .await?
        .intent_id();
    let allocation = scoped.session_header_allocation(intent).await?;
    assert_eq!(allocation.target(), 1);
    if uncertain_poll {
        let poll = scoped.begin_zero_header_poll(intent, &allocation).await?;
        scoped.mark_session_poll_uncertain(intent, poll).await?;
    }
    Ok(intent)
}

async fn retire_after_set_delete(
    fixture: &Fixture,
    intent: i64,
) -> TestResult<(Option<SessionDeleteOutcome>, Answer)> {
    let mut transport = Answer::new(404);
    let outcome = retire_session_of_deleted_set_with(
        &mut transport,
        "admin-token",
        &queue_session()?,
        &fixture.scoped,
        intent,
    )
    .await;
    Ok((outcome, transport))
}

#[tokio::test]
async fn a_deleted_set_with_a_positive_header_retires_without_polling_it() -> TestResult {
    for uncertain_poll in [false, true] {
        let fixture = fixture(true).await?;
        let intent = positive_header_session(&fixture, uncertain_poll).await?;
        record_set_delete(&fixture, "deleted").await?;
        let (outcome, transport) = retire_after_set_delete(&fixture, intent).await?;
        assert_eq!(
            outcome,
            Some(SessionDeleteOutcome::Deleted),
            "{uncertain_poll}"
        );
        assert_eq!(
            transport.seen,
            vec![(Method::Delete, transport.seen[0].1.clone())],
            "{uncertain_poll}"
        );
        assert!(transport.seen[0].1.ends_with("/10/sessions/session-1"));
        assert!(
            fixture
                .journal
                .retire_connection_if_drained(&fixture.binding)
                .await?,
            "{uncertain_poll}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_set_not_recorded_deleted_is_left_to_the_normal_poll() -> TestResult {
    for state in [None, Some("requested"), Some("uncertain"), Some("prepared")] {
        let fixture = fixture(true).await?;
        let intent = positive_header_session(&fixture, false).await?;
        if let Some(state) = state {
            record_set_delete(&fixture, state).await?;
        }
        let (outcome, transport) = retire_after_set_delete(&fixture, intent).await?;
        assert_eq!(outcome, None, "{state:?}");
        assert!(transport.seen.is_empty(), "{state:?}");
    }
    Ok(())
}
