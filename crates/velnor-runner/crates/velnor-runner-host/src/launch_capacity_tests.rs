//! Admission for one poll. `poll_and_drive` calls `admit`.

use crate::launch::{
    Admit, Idle, Seat, admit, install_job_capacity, job_capacity, needs_running,
    parse_admit_target, parse_job_capacity, poll_limit, statistics_blocked, wide_poll_limit,
};

fn decide(capacity: u32, started: u32, running: u32, idle: Idle) -> Admit {
    seat(
        capacity,
        capacity,
        started,
        running,
        running,
        u32::MAX,
        idle,
    )
}

fn decide_at(capacity: u32, target: u32, started: u32, running: u32, idle: Idle) -> Admit {
    seat(capacity, target, started, running, running, u32::MAX, idle)
}

fn seat(
    capacity: u32,
    target: u32,
    started: u32,
    occupied: u32,
    running: u32,
    assigned: u32,
    idle: Idle,
) -> Admit {
    admit(Seat {
        capacity,
        target,
        started,
        occupied,
        running,
        assigned,
        idle,
        progress: false,
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
    assert!(needs_running(Idle::Launch));
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
fn full_capacity_holds_an_uncovered_scale_offer() {
    assert_eq!(decide(2, 0, 2, Idle::Launch), Admit::Hold);
    assert_eq!(decide(2, 0, 2, Idle::Scale), Admit::Hold);
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
            occupied: 1,
            running: 1,
            assigned: 1,
            idle: Idle::Scale,
            progress: false,
        }),
        Admit::Ack { stop: false }
    );
    assert_eq!(
        admit(Seat {
            capacity: 2,
            target: 2,
            started: 1,
            occupied: 1,
            running: 1,
            assigned: 3,
            idle: Idle::Scale,
            progress: false,
        }),
        Admit::Start { stop: true }
    );
    assert_eq!(
        admit(Seat {
            capacity: 2,
            target: 2,
            started: 0,
            occupied: 0,
            running: 0,
            assigned: 1,
            idle: Idle::Scale,
            progress: false,
        }),
        Admit::Start { stop: false }
    );
    assert_eq!(
        admit(Seat {
            capacity: 2,
            target: 2,
            started: 1,
            occupied: 0,
            running: 0,
            assigned: 1,
            idle: Idle::Scale,
            progress: false,
        }),
        Admit::Start { stop: true }
    );
}

#[test]
fn capacity_one_refills_after_the_worker_exits() {
    assert_eq!(decide(1, 1, 0, Idle::Launch), Admit::Start { stop: true });
    assert_eq!(decide(1, 1, 1, Idle::Launch), Admit::Stop);
    assert!(needs_running(Idle::Launch));
    assert!(!needs_running(Idle::Empty));
    assert_eq!(decide(1, 1, 0, Idle::Scale), Admit::Start { stop: true });
    assert_eq!(decide(1, 0, 1, Idle::Scale), Admit::Hold);
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
    assert_eq!(parse_job_capacity(Some("99")), 99);
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
    assert_eq!(job_capacity(), 99);
    drop(wide);
    assert_eq!(job_capacity(), baseline);
}

#[test]
fn nested_capacity_overrides_restore_the_outer_limit() {
    let outer = install_job_capacity(3);
    assert_eq!(job_capacity(), 3);
    {
        let _inner = install_job_capacity(2);
        assert_eq!(job_capacity(), 2);
    }
    assert_eq!(job_capacity(), 3);
    drop(outer);
    assert_eq!(
        job_capacity(),
        parse_job_capacity(std::env::var("VELNOR_MAX_JOBS").ok().as_deref())
    );
}

#[test]
fn target_above_capacity_queues_the_third_job() {
    assert!(needs_running(Idle::Launch));
    assert!(!needs_running(Idle::Empty));
    assert!(!needs_running(Idle::Ack));
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
        Admit::Start { stop: true }
    );
    assert_eq!(
        decide_at(2, 3, 2, 1, Idle::Scale),
        Admit::Start { stop: true }
    );
    assert_eq!(decide_at(2, 3, 2, 2, Idle::Scale), Admit::Hold);
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
    assert_eq!(parse_admit_target(2, Some("99")), 99);
    assert_eq!(parse_admit_target(8, Some("9")), 9);
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
fn historical_starts_do_not_cover_a_free_slot() {
    assert_eq!(
        admit(Seat {
            capacity: 2,
            target: 2,
            started: 2,
            occupied: 1,
            running: 1,
            assigned: 2,
            idle: Idle::Scale,
            progress: false,
        }),
        Admit::Start { stop: true }
    );
}

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
fn uncertain_occupancy_does_not_mint_or_ack() {
    assert_eq!(
        admit(Seat {
            capacity: 1,
            target: 1,
            started: 0,
            occupied: 1,
            running: 0,
            assigned: 1,
            idle: Idle::Scale,
            progress: false,
        }),
        Admit::Hold
    );
}

#[test]
fn statistics_only_block_when_population_or_capacity_is_covered() {
    assert!(statistics_blocked(0, 0, 2, 0));
    assert!(statistics_blocked(2, 0, 2, 3));
    assert!(statistics_blocked(0, 2, 2, 3));
    assert!(statistics_blocked(0, 1, 2, 1));
    assert!(!statistics_blocked(1, 1, 2, 3));
}

#[test]
fn three_waves_refill_after_exit_and_a_mid_wave_failure() {
    for _wave in 0..3 {
        let mut running = 0u32;
        let mut started = 0u32;
        while running < 4 {
            let decision = seat(4, 4, started, running, running, 12, Idle::Launch);
            assert!(matches!(decision, Admit::Start { .. }));
            running = running.saturating_add(1);
            started = started.saturating_add(1);
        }
        assert_eq!(
            seat(4, 4, started, running, running, 12, Idle::Launch),
            Admit::Stop
        );
        running = running.saturating_sub(1);
        assert!(matches!(
            seat(4, 4, started, running, running, 12, Idle::Launch),
            Admit::Start { .. }
        ));
        started = started.saturating_add(1);
        assert_eq!(seat(4, 4, started, 0, 0, 12, Idle::Empty), Admit::Stop);
    }
}
