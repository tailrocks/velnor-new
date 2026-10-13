use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    let error = output.expect_err("one byte beyond the limit must fail");
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

#[test]
fn timeout_reaps_direct_child() -> Result<(), Box<dyn std::error::Error>> {
    let pid_file = temp_path("pid");
    let mut command = Command::new("sh");
    command
        .args(["-c", "printf '%s' \"$$\" > \"$1\"; exec sleep 10", "sh"])
        .arg(&pid_file);
    let result = run_bounded(&mut command, 1_024, Duration::from_millis(200));
    assert!(result.is_err());
    let pid = std::fs::read_to_string(&pid_file)?;
    assert!(
        !process_alive(pid.trim())?,
        "timed-out child {pid} still exists"
    );
    std::fs::remove_file(pid_file)?;
    Ok(())
}

#[test]
fn timeout_kills_descendants_holding_the_capture_pipe() {
    let marker = temp_path("descendant");
    let mut command = Command::new("sh");
    command
        .args(["-c", "(sleep 0.4; printf survived > \"$1\") & exit 0", "sh"])
        .arg(marker.as_os_str());
    let result = run_bounded(&mut command, 1_024, Duration::from_millis(200));
    assert!(result.is_err());
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        !marker.exists(),
        "descendant survived the process-group kill"
    );
}

fn process_alive(pid: &str) -> Result<bool, std::io::Error> {
    let status = Command::new("sh")
        .args(["-c", "kill -0 \"$1\" >/dev/null 2>&1", "sh", pid])
        .status()?;
    Ok(status.success())
}

fn temp_path(label: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "velnor-bounded-{label}-{}-{nonce}",
        std::process::id()
    ))
}
