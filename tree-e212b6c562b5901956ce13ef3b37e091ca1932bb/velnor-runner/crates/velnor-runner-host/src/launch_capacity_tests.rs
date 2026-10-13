//! Admission for one poll. `poll_and_drive` calls `admit`.

use crate::IntentState;
use crate::launch::{
    Admit, Idle, Seat, admit, needs_running, occupies, parse_job_capacity, poll_limit,
};

fn decide(capacity: u32, started: u32, running: u32, idle: Idle) -> Admit {
    admit(Seat {
        capacity,
        started,
        running,
        idle,
    })
}

#[test]
fn capacity_one_first_launch_starts_then_stops() {
    assert_eq!(decide(1, 0, 0, Idle::Launch), Admit::Start { stop: true });
}

#[test]
fn capacity_two_first_launch_starts_and_stays() {
    assert_eq!(decide(2, 0, 0, Idle::Launch), Admit::Start { stop: false });
}

#[test]
fn capacity_two_empty_after_one_start_stays() {
    assert_eq!(decide(2, 1, 0, Idle::Empty), Admit::Stay);
    assert_eq!(decide(2, 1, 1, Idle::Empty), Admit::Stay);
    assert_eq!(decide(1, 1, 1, Idle::Empty), Admit::Stop);
}

#[test]
fn capacity_two_second_launch_starts() {
    assert!(needs_running(2, 1, Idle::Launch));
    assert_eq!(decide(2, 1, 1, Idle::Launch), Admit::Start { stop: true });
    assert_eq!(decide(2, 1, 0, Idle::Launch), Admit::Start { stop: true });
}

#[test]
fn blocked_poll_is_an_error() {
    assert_eq!(decide(1, 0, 0, Idle::Blocked), Admit::Error);
    assert_eq!(decide(2, 1, 0, Idle::Blocked), Admit::Error);
}

#[test]
fn full_launch_is_not_started_or_acked() {
    assert_eq!(decide(1, 0, 1, Idle::Launch), Admit::Hold);
    assert_eq!(decide(2, 1, 2, Idle::Launch), Admit::Hold);
    assert_eq!(decide(8, 3, 8, Idle::Launch), Admit::Hold);
}

#[test]
fn ack_does_not_start_or_free_a_slot() {
    let running = 2;
    assert_eq!(decide(1, 0, 1, Idle::Ack), Admit::Ack { stop: false });
    assert_eq!(decide(1, 1, 1, Idle::Ack), Admit::Ack { stop: true });
    assert_eq!(decide(2, 1, running, Idle::Ack), Admit::Ack { stop: false });
    assert_eq!(decide(2, 0, running, Idle::Launch), Admit::Hold);
}

#[test]
fn capacity_one_after_start_keeps_the_old_loop() {
    assert_eq!(decide(1, 1, 0, Idle::Launch), Admit::Stop);
    assert_eq!(decide(1, 1, 1, Idle::Launch), Admit::Stop);
    assert!(!needs_running(1, 1, Idle::Launch));
    assert_eq!(decide(1, 1, 0, Idle::Scale), Admit::Ack { stop: true });
    assert_eq!(decide(1, 0, 1, Idle::Scale), Admit::Ack { stop: true });
    assert_eq!(decide(1, 0, 0, Idle::Scale), Admit::Start { stop: true });
    assert_eq!(decide(1, 0, 0, Idle::Empty), Admit::Stay);
}

#[test]
fn job_capacity_parser_bounds() {
    assert_eq!(parse_job_capacity(None), 1);
    assert_eq!(parse_job_capacity(Some("")), 1);
    assert_eq!(parse_job_capacity(Some("   ")), 1);
    assert_eq!(parse_job_capacity(Some("nope")), 1);
    assert_eq!(parse_job_capacity(Some("0")), 1);
    assert_eq!(parse_job_capacity(Some("2")), 2);
    assert_eq!(parse_job_capacity(Some(" 2 ")), 2);
    assert_eq!(parse_job_capacity(Some("8")), 8);
    assert_eq!(parse_job_capacity(Some("99")), 8);
}

#[test]
fn poll_limit_follows_capacity() {
    assert_eq!(poll_limit(1, None), 8);
    assert_eq!(poll_limit(2, None), 8);
    assert_eq!(poll_limit(1, Some("nope")), 8);
    assert_eq!(poll_limit(1, Some("")), 8);
    assert_eq!(poll_limit(1, Some("0")), 1);
    assert_eq!(poll_limit(1, Some("24")), 8);
    assert_eq!(poll_limit(2, Some("24")), 24);
    assert_eq!(poll_limit(2, Some("25")), 24);
    assert_eq!(poll_limit(2, Some("9")), 9);
}

#[test]
fn occupies_only_a_running_named_container() {
    let id = Some("abc");
    assert!(occupies(IntentState::Done, id, true));
    assert!(occupies(IntentState::Pending, id, true));
    assert!(occupies(IntentState::Uncertain, id, true));
    assert!(!occupies(IntentState::Done, id, false));
    assert!(!occupies(IntentState::Failed, id, true));
    assert!(!occupies(IntentState::Pending, None, true));
    assert!(!occupies(IntentState::Failed, None, false));
}
