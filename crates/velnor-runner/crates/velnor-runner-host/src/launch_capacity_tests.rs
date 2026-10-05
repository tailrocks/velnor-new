//! Admission for one poll. `poll_and_drive` calls `admit`.

use crate::IntentState;
use crate::launch::{
    Admit, Idle, Seat, admit, install_job_capacity, job_capacity, needs_running, occupies,
    parse_admit_target, parse_job_capacity, poll_limit, wide_poll_limit,
};

fn decide(capacity: u32, started: u32, running: u32, idle: Idle) -> Admit {
    admit(Seat {
        capacity,
        target: capacity,
        started,
        running,
        assigned: u32::MAX,
        idle,
    })
}

fn decide_at(capacity: u32, target: u32, started: u32, running: u32, idle: Idle) -> Admit {
    admit(Seat {
        capacity,
        target,
        started,
        running,
        assigned: u32::MAX,
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
    assert!(needs_running(2, 2, 1, Idle::Launch));
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
fn full_capacity_holds_job_offers_but_keeps_aggregate_scale_ack_semantics() {
    assert_eq!(decide(2, 0, 2, Idle::Launch), Admit::Hold);
    assert_eq!(decide(2, 0, 2, Idle::Scale), Admit::Ack { stop: false });
    assert_eq!(decide(2, 0, 1, Idle::Scale), Admit::Start { stop: false });
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
fn scale_does_not_mint_again_once_assigned_is_covered() {
    assert_eq!(
        admit(Seat {
            capacity: 2,
            target: 2,
            started: 1,
            running: 1,
            assigned: 1,
            idle: Idle::Scale,
        }),
        Admit::Ack { stop: false }
    );
    assert_eq!(
        admit(Seat {
            capacity: 2,
            target: 2,
            started: 1,
            running: 1,
            assigned: 3,
            idle: Idle::Scale,
        }),
        Admit::Start { stop: true }
    );
    assert_eq!(
        admit(Seat {
            capacity: 2,
            target: 2,
            started: 0,
            running: 0,
            assigned: 1,
            idle: Idle::Scale,
        }),
        Admit::Start { stop: false }
    );
    assert_eq!(
        admit(Seat {
            capacity: 2,
            target: 2,
            started: 1,
            running: 0,
            assigned: 1,
            idle: Idle::Scale,
        }),
        Admit::Start { stop: true }
    );
}

#[test]
fn capacity_one_after_start_keeps_the_old_loop() {
    assert_eq!(decide(1, 1, 0, Idle::Launch), Admit::Stop);
    assert_eq!(decide(1, 1, 1, Idle::Launch), Admit::Stop);
    assert!(!needs_running(1, 1, 1, Idle::Launch));
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
fn installed_capacity_beats_the_env_parse() {
    let baseline = parse_job_capacity(std::env::var("VELNOR_MAX_JOBS").ok().as_deref());
    assert_eq!(job_capacity(), baseline);
    let chosen = if baseline == 4 { 5 } else { 4 };
    let guard = install_job_capacity(chosen);
    assert_eq!(job_capacity(), chosen);
    drop(guard);
    assert_eq!(job_capacity(), baseline);
    let wide = install_job_capacity(99);
    assert_eq!(job_capacity(), 8);
    drop(wide);
    assert_eq!(job_capacity(), baseline);
}

#[test]
fn target_above_capacity_queues_the_third_job() {
    assert!(needs_running(2, 3, 2, Idle::Launch));
    assert!(!needs_running(2, 3, 3, Idle::Launch));
    assert!(!needs_running(2, 3, 2, Idle::Empty));
    assert_eq!(decide_at(2, 3, 2, 2, Idle::Launch), Admit::Hold);
    assert_eq!(
        decide_at(2, 3, 1, 0, Idle::Launch),
        Admit::Start { stop: false }
    );
    assert_eq!(
        decide_at(2, 3, 2, 1, Idle::Launch),
        Admit::Start { stop: true }
    );
    assert_eq!(decide_at(2, 3, 2, 2, Idle::Empty), Admit::Stay);
    assert_eq!(decide_at(2, 3, 3, 1, Idle::Empty), Admit::Stop);
    assert_eq!(decide_at(2, 3, 2, 2, Idle::Ack), Admit::Ack { stop: false });
    assert_eq!(decide_at(2, 3, 3, 1, Idle::Ack), Admit::Ack { stop: true });
    assert_eq!(
        decide_at(2, 3, 3, 0, Idle::Scale),
        Admit::Ack { stop: true }
    );
    assert_eq!(
        decide_at(2, 3, 2, 1, Idle::Scale),
        Admit::Start { stop: true }
    );
    assert_eq!(
        decide_at(2, 3, 2, 2, Idle::Scale),
        Admit::Ack { stop: false }
    );
}

#[test]
fn admit_target_parser_bounds() {
    assert_eq!(parse_admit_target(2, None), 2);
    assert_eq!(parse_admit_target(2, Some("")), 2);
    assert_eq!(parse_admit_target(2, Some("   ")), 2);
    assert_eq!(parse_admit_target(2, Some("nope")), 2);
    assert_eq!(parse_admit_target(2, Some("0")), 2);
    assert_eq!(parse_admit_target(2, Some("1")), 2);
    assert_eq!(parse_admit_target(2, Some("2")), 2);
    assert_eq!(parse_admit_target(2, Some("3")), 3);
    assert_eq!(parse_admit_target(2, Some(" 3 ")), 3);
    assert_eq!(parse_admit_target(2, Some("8")), 8);
    assert_eq!(parse_admit_target(2, Some("99")), 8);
    assert_eq!(parse_admit_target(8, Some("9")), 8);
}

#[test]
fn wide_poll_limit_defaults_to_ninety() {
    assert_eq!(wide_poll_limit(None), 90);
    assert_eq!(wide_poll_limit(Some("nope")), 90);
    assert_eq!(wide_poll_limit(Some("")), 90);
    assert_eq!(wide_poll_limit(Some("0")), 1);
    assert_eq!(wide_poll_limit(Some("90")), 90);
    assert_eq!(wide_poll_limit(Some("120")), 120);
    assert_eq!(wide_poll_limit(Some("121")), 120);
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
