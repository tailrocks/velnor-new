//! Real process witnesses for the strict group-drain route.

use super::*;
use crate::{CheckDeadline, IsolatedCommand, MiseError, command::CancelHandle};
use rustix::process::{Signal, kill_process, kill_process_group};
use std::{
    ffi::OsString,
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn task(script: &str) -> IsolatedCommand {
    IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from(script)],
        &[],
    )
    .expect("fixed shell command")
}

#[test]
fn strict_success_kills_delayed_group_descendant_before_receipt() {
    let root = crate::test_temp_dir::TestTempDir::new("strict-drain").expect("temporary root");
    let marker = root.path().join("late-marker");
    let script = format!(
        "(/bin/sleep 0.25; /usr/bin/touch '{}') >/dev/null 2>&1 & exit 0",
        marker.display()
    );
    let (output, _receipt) = task(&script)
        .run_quiescent_until(
            4096,
            CheckDeadline::after(Duration::from_secs(2)).expect("valid test deadline"),
        )
        .expect("leader and group drained");
    assert!(output.success);
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        !marker.exists(),
        "group descendant survived strict completion"
    );
}

#[test]
fn strict_timeout_fails_without_a_drain_receipt() {
    let error = task("/bin/sleep 2")
        .run_quiescent_until(
            4096,
            CheckDeadline::after(Duration::from_millis(350)).expect("valid test deadline"),
        )
        .expect_err("deadline must abort child");
    assert!(matches!(error, MiseError::SpawnFailed { .. }));
}

#[test]
fn ordinary_timeout_gets_cleanup_budget_after_long_running_work() {
    let error = task("/bin/sleep 2")
        .run_cancellable(4096, Duration::from_millis(400), &CancelHandle::new())
        .expect_err("deadline must stop the command");
    match error {
        MiseError::SpawnFailed { message, .. } => {
            assert!(message.contains("timeout_after_secs"));
            assert!(
                !message.contains("cleanup_failed"),
                "cleanup budget expired early"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn anchor_loss_returns_without_a_drain_receipt() {
    let anchor = ProcessGroupAnchor::spawn().expect("live anchor");
    kill_process_group(anchor.pid, Signal::KILL).expect("kill owned anchor group");
    let mut exited = false;
    for _ in 0..20 {
        if observe_exit(&anchor.child)
            .expect("anchor remains waitable")
            .is_some()
        {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        exited,
        "killed anchor should be observed before drain attempt"
    );
    let error = anchor
        .drain(None, Instant::now() + Duration::from_secs(1))
        .expect_err("lost anchor cannot certify group absence");
    assert_eq!(error, "anchor_not_live");
}

#[test]
fn terminal_unreaped_anchor_kills_group_descendants_before_reaping() {
    let root = crate::test_temp_dir::TestTempDir::new("terminal-anchor").expect("temporary root");
    let marker = root.path().join("late-marker");
    let anchor = ProcessGroupAnchor::spawn().expect("live anchor");
    let mut target = grouped_task(
        &format!(
            "(/bin/sleep 0.25; /usr/bin/touch '{}') >/dev/null 2>&1 & wait",
            marker.display()
        ),
        anchor.pgid(),
    );

    kill_process(anchor.pid, Signal::KILL).expect("kill anchor only");
    wait_for_terminal_anchor(&anchor.child);
    let error = anchor
        .drain(Some(&mut target), Instant::now() + Duration::from_secs(1))
        .expect_err("terminal anchor cannot certify group drain");
    assert_eq!(error, "anchor_not_live");
    std::thread::sleep(Duration::from_millis(350));
    assert!(!marker.exists(), "target descendant survived group drain");
}

#[test]
fn lost_wait_ownership_never_signals_the_old_process_group() {
    let root = crate::test_temp_dir::TestTempDir::new("lost-anchor").expect("temporary root");
    let marker = root.path().join("surviving-marker");
    let mut anchor = ProcessGroupAnchor::spawn().expect("live anchor");
    let mut target = grouped_task(
        &format!(
            "(/bin/sleep 0.2; /usr/bin/touch '{}') >/dev/null 2>&1 & wait",
            marker.display()
        ),
        anchor.pgid(),
    );

    kill_process(anchor.pid, Signal::KILL).expect("kill anchor only");
    anchor
        .child
        .wait()
        .expect("reap anchor outside exclusive waiter");
    let error = anchor
        .drain(Some(&mut target), Instant::now() + Duration::from_secs(1))
        .expect_err("lost wait ownership cannot certify group drain");
    assert_eq!(error, "anchor_wait_ownership_lost");
    assert!(
        marker.exists(),
        "target proves no old-group signal was sent"
    );
}

#[test]
fn strict_cancellation_and_output_cap_failure_return_errors() {
    let cancel = CancelHandle::new();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(80));
            cancel.cancel();
        });
        let error = task("/bin/sleep 2")
            .run_quiescent_cancellable_until(
                4096,
                CheckDeadline::after(Duration::from_secs(2)).expect("valid test deadline"),
                &cancel,
            )
            .expect_err("cancellation must abort strict run");
        assert!(matches!(error, MiseError::SpawnFailed { .. }));
    });

    let error = task("printf 123456789")
        .run_quiescent_until(
            3,
            CheckDeadline::after(Duration::from_secs(2)).expect("valid test deadline"),
        )
        .expect_err("output cap must abort strict run");
    assert!(matches!(error, MiseError::SpawnFailed { .. }));
}

#[test]
fn anchor_command_clears_the_parent_environment() {
    assert!(
        std::env::vars_os().next().is_some(),
        "test requires a nonempty parent environment"
    );
    let child = anchor_command("/usr/bin/env")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn isolated environment probe");
    let output = child.wait_with_output().expect("collect probe output");
    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "anchor inherited environment data"
    );
}

fn grouped_task(script: &str, pgid: i32) -> Child {
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(pgid);
    command.spawn().expect("spawn group member")
}

fn wait_for_terminal_anchor(child: &Child) {
    for _ in 0..100 {
        if observe_exit(child)
            .expect("anchor remains waitable")
            .is_some()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("anchor did not become terminal");
}
