use super::{SessionDriveOutcome, offers, session_stop_after_batch};

#[test]
fn unacknowledged_available_batch_stops_before_the_next_poll() {
    assert_eq!(
        session_stop_after_batch(offers::BatchOutcome::AvailableOffersHeld),
        Some(SessionDriveOutcome::AvailableOffersHeld)
    );
}

#[test]
fn only_acknowledged_batches_continue_the_poll_loop() {
    assert_eq!(
        session_stop_after_batch(offers::BatchOutcome::Advanced),
        None
    );
    assert_eq!(
        session_stop_after_batch(offers::BatchOutcome::Stopped),
        Some(SessionDriveOutcome::Stopped)
    );
}
