use std::process::Command;
use std::time::{Duration, Instant};

use super::run_bounded;

#[test]
fn bounded_capture_accepts_exact_output_and_rejects_one_extra_byte() {
    let mut exact = Command::new("sh");
    exact.args(["-c", "printf x"]);
    let output = run_bounded(&mut exact, 1, Duration::from_secs(1))
        .expect("one byte fits the capture limit");
    assert_eq!(output.stdout, b"x");

    let mut oversized = Command::new("sh");
    oversized.args(["-c", "printf xy"]);
    let output = run_bounded(&mut oversized, 1, Duration::from_secs(1));
    let error = output.err().expect("one byte beyond the limit must fail");
    assert!(error.contains("exceeds 1 bytes"), "{error}");
}

#[test]
fn timeout_covers_child_exit_and_descendant_pipe_drain() {
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 2 & exit 0"]);
    let started = Instant::now();
    let result = run_bounded(&mut command, 1_024, Duration::from_millis(200));
    let elapsed = started.elapsed();
    let error = result.expect_err("descendant-held pipes must not report success");
    assert!(error.contains("timed out"), "{error}");
    assert!(elapsed < Duration::from_millis(350), "elapsed: {elapsed:?}");
}

#[test]
fn timeout_kills_a_running_process_without_waiting_for_its_natural_exit() {
    let mut command = Command::new("sleep");
    command.arg("10");
    let started = Instant::now();
    let result = run_bounded(&mut command, 1_024, Duration::from_millis(200));
    let elapsed = started.elapsed();
    assert!(result.is_err());
    assert!(elapsed < Duration::from_millis(350), "elapsed: {elapsed:?}");
}
