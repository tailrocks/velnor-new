//! Progress notices on covered or full slots are acknowledged.

use crate::launch::{Admit, Idle, Seat, admit};

#[test]
fn full_progress_notice_is_acknowledged() {
    assert_eq!(
        admit(Seat {
            capacity: 1,
            target: 1,
            started: 0,
            occupied: 1,
            running: 0,
            assigned: 5,
            idle: Idle::Scale,
            progress: true,
        }),
        Admit::Ack { stop: false }
    );
}

#[test]
fn full_slot_progress_notice_is_acknowledged() {
    assert_eq!(
        admit(Seat {
            capacity: 4,
            target: 4,
            started: 0,
            occupied: 4,
            running: 4,
            assigned: 5,
            idle: Idle::Scale,
            progress: true,
        }),
        Admit::Ack { stop: false }
    );
    assert_eq!(
        admit(Seat {
            capacity: 4,
            target: 4,
            started: 3,
            occupied: 3,
            running: 3,
            assigned: 5,
            idle: Idle::Scale,
            progress: true,
        }),
        Admit::Start { stop: true }
    );
}
