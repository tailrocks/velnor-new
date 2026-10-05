#![cfg(unix)]

use std::cell::Cell;
use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use super::last_chars;
use super::process::{
    RunLimits, cleanup_failed_run, isolate_process_tree, run_bounded, set_nonblocking, take_output,
};

fn spawn_shell(script: &str) -> io::Result<Child> {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_process_tree(&mut command);
    command.spawn()
}

fn limits(runtime: Duration, output_drain: Duration) -> RunLimits {
    RunLimits {
        runtime,
        output_drain,
        cleanup: Duration::from_secs(1),
    }
}

#[test]
fn direct_exit_with_descendant_held_pipes_finishes_after_tree_cleanup() -> io::Result<()> {
    let mut child =
        spawn_shell("sleep 30 & descendant=$!; printf '%s\\n' \"$descendant\"; exit 0")?;
    let started = Instant::now();
    let output = run_bounded(
        &mut child,
        limits(Duration::from_secs(2), Duration::from_millis(80)),
    )?;

    assert!(output.status.is_some_and(|status| status.success()));
    assert!(!String::from_utf8_lossy(&output.streams.0).trim().is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn direct_timeout_kills_child_tree_and_returns_within_cleanup_bound() -> io::Result<()> {
    let mut child = spawn_shell("sleep 30 & wait")?;
    let started = Instant::now();
    let output = run_bounded(
        &mut child,
        limits(Duration::from_millis(120), Duration::from_millis(80)),
    )?;

    assert!(
        output.status.is_none(),
        "timeout keeps the existing timeout status"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn threaded_reader_fallback_bounds_descendant_held_pipes() -> io::Result<()> {
    let mut child =
        spawn_shell("sleep 30 & descendant=$!; printf '%s\\n' \"$descendant\"; exit 0")?;
    let started = Instant::now();
    let output = super::readers::run_threaded_bounded(
        &mut child,
        limits(Duration::from_secs(2), Duration::from_millis(80)),
    )?;

    assert!(output.status.is_some_and(|status| status.success()));
    assert!(!String::from_utf8_lossy(&output.streams.0).trim().is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn threaded_reader_fallback_bounds_timed_out_child_tree() -> io::Result<()> {
    let mut child = spawn_shell("sleep 30 & wait")?;
    let started = Instant::now();
    let output = super::readers::run_threaded_bounded(
        &mut child,
        limits(Duration::from_millis(120), Duration::from_millis(80)),
    )?;

    assert!(output.status.is_none());
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn wait_error_path_cleans_child_tree_without_joining_blocked_readers() -> io::Result<()> {
    let mut child = spawn_shell("sleep 30 & wait")?;
    let (stdout, stderr) = take_output(&mut child)?;
    let mut stdout = Some(stdout);
    let mut stderr = Some(stderr);
    set_nonblocking(
        stdout
            .as_ref()
            .ok_or_else(|| io::Error::other("stdout pipe missing"))?,
    )?;
    set_nonblocking(
        stderr
            .as_ref()
            .ok_or_else(|| io::Error::other("stderr pipe missing"))?,
    )?;
    thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    let source = io::Error::other("injected try_wait failure");
    let error = cleanup_failed_run(
        &mut child,
        &mut stdout,
        &mut stderr,
        &mut (Vec::new(), Vec::new()),
        &source,
        Duration::from_secs(1),
        true,
    );

    assert!(error.to_string().contains("injected try_wait failure"));
    assert!(child.try_wait()?.is_some());
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn nonzero_direct_exit_keeps_diagnostic_tail() -> io::Result<()> {
    let mut child = spawn_shell("printf 'deny diagnostic'; exit 7")?;
    let output = run_bounded(
        &mut child,
        limits(Duration::from_secs(2), Duration::from_millis(80)),
    )?;

    assert!(output.status.is_some_and(|status| !status.success()));
    assert_eq!(
        String::from_utf8_lossy(&output.streams.0),
        "deny diagnostic"
    );
    Ok(())
}

#[test]
fn captured_diagnostic_tail_uses_unicode_characters() {
    assert_eq!(last_chars("αβγδε", 3), "γδε");
    assert_eq!(last_chars("short", 500), "short");
}

#[test]
fn nonblocking_drain_bounds_repeated_interrupted_reads_per_poll() -> io::Result<()> {
    struct InterruptedReader(Rc<Cell<usize>>);

    impl Read for InterruptedReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            let calls = self.0.get() + 1;
            self.0.set(calls);
            if calls > super::process::INTERRUPTED_READ_LIMIT + 1 {
                Ok(0)
            } else {
                Err(io::Error::from(io::ErrorKind::Interrupted))
            }
        }
    }

    let calls = Rc::new(Cell::new(0));
    let mut stream = Some(InterruptedReader(Rc::clone(&calls)));
    let mut output = Vec::new();
    super::process::drain_stream(&mut stream, &mut output)?;

    assert_eq!(calls.get(), super::process::INTERRUPTED_READ_LIMIT);
    assert!(stream.is_some());
    Ok(())
}
