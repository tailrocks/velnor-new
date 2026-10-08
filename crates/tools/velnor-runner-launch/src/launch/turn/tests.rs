//! Regressions for the same start path used by `Turn::start`.

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail, create_session,
};

use super::{HeldOffers, Ready, StartOutcome};

#[test]
fn held_offer_retains_session_until_that_message_is_acked() {
    let polled = crate::launch::harness::available(&[3]);
    let message_id = match &polled {
        Poll::Batch(batch) => batch.message_id,
        Poll::Empty => return,
    };
    let mut held = super::HeldOffers::default();

    held.observe_batch(&polled);
    assert!(held.requires_retention());
    held.observe_ack(Some(message_id.saturating_add(1)));
    assert!(held.requires_retention());
    held.observe_ack(Some(message_id));
    assert!(!held.requires_retention());
}

#[tokio::test]
async fn capacity_stop_on_batch_preserves_session_without_delete() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use velnor_runner_host::scale_set::EnsureError;

    let polled = crate::launch::harness::available(&[3]);
    let decision = crate::launch::capacity::admit(crate::launch::capacity::Seat {
        capacity: 1,
        target: 1,
        started: 1,
        occupied: 1,
        running: 1,
        assigned: 0,
        idle: crate::launch::steps::Idle::Launch,
        progress: false,
    });
    assert_eq!(decision, crate::launch::Admit::Stop);

    let mut held = HeldOffers::default();
    held.observe_batch(&polled);
    let delete_calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&delete_calls);
    let poll: Result<(), EnsureError> = Ok(());
    crate::launch::session::close_after_poll(&poll, held.requires_retention(), || async move {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .await
    .expect("session close decision");

    assert_eq!(delete_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn start_that_returns_without_ack_preserves_batch_and_session() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use velnor_runner_host::scale_set::EnsureError;

    let polled = crate::launch::harness::available(&[3]);
    let outcome = StartOutcome {
        stop: true,
        acknowledged_message_id: None,
    };
    let mut held = HeldOffers::default();
    held.observe_batch(&polled);
    held.observe_ack(outcome.acknowledged_message_id);
    assert!(outcome.stop);

    let delete_calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&delete_calls);
    let poll: Result<(), EnsureError> = Ok(());
    crate::launch::session::close_after_poll(&poll, held.requires_retention(), || async move {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .await
    .expect("session close decision");

    assert_eq!(delete_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn empty_stop_remains_closeable() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use velnor_runner_host::scale_set::EnsureError;

    let polled = Poll::Empty;
    let mut held = HeldOffers::default();
    held.observe_batch(&polled);
    let delete_calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&delete_calls);
    let poll: Result<(), EnsureError> = Ok(());
    crate::launch::session::close_after_poll(&poll, held.requires_retention(), || async move {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .await
    .expect("session close decision");

    assert_eq!(delete_calls.load(Ordering::SeqCst), 1);
}

const INITIAL_SESSION: &[u8] = br#"{"sessionId":"session","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token","statistics":{"totalAvailableJobs":0,"totalAcquiredJobs":0,"totalAssignedJobs":0,"totalRunningJobs":0,"totalRegisteredRunners":0,"totalBusyRunners":0,"totalIdleRunners":0}}"#;

struct InitialSession;

impl Transport for InitialSession {
    fn exchange(&mut self, _request: &SessionRequest) -> Result<Exchange, TransportFail> {
        Ok(Exchange {
            status: 200,
            body: INITIAL_SESSION.to_vec(),
        })
    }
}

pub(super) fn zero_assignment_session() -> Result<QueueSession, String> {
    create_session(&mut InitialSession, 1, "owner", "admin-token")
        .map_err(|error| error.to_string())
}

pub(super) fn ready<'a>(session: &'a QueueSession, polled: &'a Poll) -> Ready<'a> {
    Ready {
        set_id: 1,
        queue_token: session.token().to_owned(),
        admin_token: "admin-token",
        path: "messages".to_owned(),
        polled,
    }
}

#[cfg(all(test, unix))]
mod admission_volume_tests;
#[cfg(all(test, unix))]
mod effect_replay;
#[cfg(all(test, unix))]
mod legacy_failed;
mod observations;
#[cfg(all(test, unix))]
mod progress_tests;
mod pump_tests;
#[cfg(all(test, unix))]
mod start_tests;
