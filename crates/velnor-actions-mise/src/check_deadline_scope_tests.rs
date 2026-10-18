use super::CheckDeadline;
use crate::{
    IsolatedCommand, MiseError,
    command::{SPAWN_DEADLINE_MESSAGE, SPAWN_TIMEOUT_MESSAGE_PREFIX, is_cancel_or_timeout},
};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};

fn budget(milliseconds: u64) -> CheckDeadline {
    CheckDeadline::after(Duration::from_millis(milliseconds)).expect("deadline")
}

fn command(script: &str) -> IsolatedCommand {
    IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from(script)],
        &[],
    )
    .expect("command")
}

fn assert_scoped_command_timeout(error: &MiseError) {
    assert!(
        matches!(error, MiseError::SpawnFailed { message, .. }
        if is_cancel_or_timeout(error) && message == SPAWN_DEADLINE_MESSAGE),
        "expected the effective operation's absolute deadline, got {error:?}"
    );
}

#[test]
fn nested_scopes_only_shorten_and_restore_the_parent() {
    let outer = budget(5000);
    outer
        .scope(|| {
            let before = outer.remaining().expect("outer remaining");
            budget(10000)
                .scope(|| {
                    assert!(budget(10000).remaining().expect("nested remaining") <= before);
                })
                .expect("longer scope");
            budget(100)
                .scope(|| {
                    assert!(
                        outer.remaining().expect("short remaining") <= Duration::from_millis(100)
                    );
                })
                .expect("shorter scope");
            assert!(outer.remaining().expect("restored parent") > Duration::from_secs(1));
        })
        .expect("outer scope");
    assert!(CheckDeadline::check_scope().is_ok());
}

#[test]
fn scope_restores_after_success_error_and_panic() {
    assert_eq!(budget(1000).scope(|| 42).expect("success"), 42);
    let failure = budget(1000)
        .scope(|| Err::<(), _>("fixture failure"))
        .expect("scope");
    assert_eq!(failure, Err("fixture failure"));
    let panic = std::panic::catch_unwind(|| {
        drop(budget(1000).scope(|| panic!("fixture unwind")));
    });
    assert!(panic.is_err());
    assert!(budget(10000).remaining().expect("restored") > Duration::from_secs(5));
    assert!(CheckDeadline::check_scope().is_ok());
}

#[test]
fn elapsed_setup_and_retries_cannot_renew_default_command_budgets() {
    let started = Instant::now();
    let result = budget(250).scope(|| {
        command("sleep 0.10").run().expect("setup");
        let failed = command("sleep 5")
            .run()
            .expect_err("remaining budget expires");
        assert_scoped_command_timeout(&failed);
        command("exit 0")
            .run()
            .expect_err("retry cannot renew expired budget");
    });
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(CheckDeadline::check_scope().is_ok());
}

#[test]
fn expired_scope_rejects_before_spawning_and_late_success_is_rejected() {
    let root = crate::test_temp_dir::TestTempDir::new("velnor-deadline-scope").expect("root");
    let marker = root.path().join("spawned");
    let expired = CheckDeadline::from_start(
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("expired fixture start"),
        Duration::from_millis(1),
    )
    .expect("past deadline");
    assert!(
        expired
            .scope(|| command(&format!("touch '{}'", marker.display())).run())
            .is_err()
    );
    assert!(!marker.exists());
    assert!(
        budget(10)
            .scope(|| std::thread::sleep(Duration::from_millis(30)))
            .is_err()
    );
    assert!(CheckDeadline::check_scope().is_ok());
}

#[test]
fn scope_is_synchronous_and_does_not_change_another_threads_budget() {
    budget(100)
        .scope(|| {
            std::thread::spawn(|| {
                assert!(
                    budget(5000).remaining().expect("independent thread") > Duration::from_secs(1)
                );
            })
            .join()
            .expect("worker");
        })
        .expect("scope");
}

#[test]
fn scoped_default_command_kills_pipe_holding_descendants_at_outer_deadline() {
    let root = crate::test_temp_dir::TestTempDir::new("velnor-deadline-scope").expect("root");
    let marker = root.path().join("descendant-survived");
    let script = format!(
        "(sleep 0.40; printf leaked > '{}') & exit 0",
        marker.display()
    );
    let started = Instant::now();
    assert!(budget(100).scope(|| command(&script).run()).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !marker.exists(),
        "scope expiration must kill the descendant group"
    );
}

#[test]
fn cleanup_reserve_uses_exact_runner_grace_from_the_original_start() {
    let started = Instant::now();
    let full = Duration::from_secs(300);
    let deadline = CheckDeadline::from_start_with_cleanup_reserve(started, full).expect("budget");
    assert_eq!(
        deadline.0.duration_since(started) + IsolatedCommand::PROCESS_CLEANUP_GRACE,
        full
    );
    let ordinary = CheckDeadline::from_start(started, full).expect("ordinary deadline");
    assert_eq!(
        ordinary.0.duration_since(started),
        full,
        "ordinary budgets are unchanged"
    );
}

#[test]
fn cleanup_reserve_rejects_insufficient_or_zero_work_budget() {
    let started = Instant::now();
    let reserve = IsolatedCommand::PROCESS_CLEANUP_GRACE;
    for budget in [
        Duration::ZERO,
        reserve
            .checked_sub(Duration::from_nanos(1))
            .expect("below-reserve fixture"),
        reserve,
    ] {
        assert!(CheckDeadline::from_start_with_cleanup_reserve(started, budget).is_err());
    }
    let deadline =
        CheckDeadline::from_start_with_cleanup_reserve(started, reserve + Duration::from_nanos(1))
            .expect("one nanosecond of work");
    assert_eq!(deadline.0.duration_since(started), Duration::from_nanos(1));
}

#[test]
fn reserved_deadline_leaves_group_cleanup_within_total_operational_budget() {
    let started = Instant::now();
    let full = Duration::from_millis(600);
    let deadline = CheckDeadline::from_start_with_cleanup_reserve(started, full).expect("deadline");
    let result = deadline.scope(|| {
        let error = command("sleep 5").run().expect_err("work deadline expires");
        assert_scoped_command_timeout(&error);
    });
    assert!(result.is_err());
    assert!(
        started.elapsed() < full + Duration::from_millis(100),
        "work timeout plus cleanup must fit the budget with scheduling tolerance"
    );
}

#[test]
fn earlier_command_timeout_preserves_its_own_diagnostic() {
    budget(3000)
        .scope(|| {
            let error = command("sleep 5")
                .run_bounded(1024, Duration::from_secs(1))
                .expect_err("command expires before outer scope");
            assert!(
                matches!(&error, MiseError::SpawnFailed { message, .. }
            if is_cancel_or_timeout(&error)
                && message.strip_prefix(SPAWN_TIMEOUT_MESSAGE_PREFIX) == Some("1")),
                "earlier command must retain its timeout diagnostic, got {error:?}"
            );
        })
        .expect("longer enclosing budget remains available");
}

#[test]
fn nested_scope_clamp_reports_absolute_deadline_not_requested_timeout() {
    let result = budget(100).scope(|| {
        let nested = budget(2000).scope(|| {
            let error = command("sleep 5")
                .run_bounded(1024, Duration::from_secs(1))
                .expect_err("outer scope expires first");
            assert_scoped_command_timeout(&error);
        });
        assert!(nested.is_err());
    });
    assert!(result.is_err());
    assert!(CheckDeadline::check_scope().is_ok());
}
