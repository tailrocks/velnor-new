use std::fs;
use std::io;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::support::{CasePaths, OwnedTempDir};

pub(super) fn run_signalled(
    mut process: Command,
    paths: &CasePaths,
    signal: &str,
    temp: &mut OwnedTempDir,
) -> Output {
    process
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process.spawn().expect("spawn signal-test shell");
    let process_group = child.id();
    temp.mark_process_group_pending();
    if let Err(error) = wait_until_ready(&mut child, &paths.ready) {
        terminate_and_panic(
            temp,
            &mut child,
            process_group,
            format!("signal fixture did not become ready: {error}"),
        );
    }
    match process_group_is_alive(process_group) {
        Ok(true) => {}
        Ok(false) => terminate_and_panic(
            temp,
            &mut child,
            process_group,
            "ready signal-test process group is already gone",
        ),
        Err(error) => terminate_and_panic(
            temp,
            &mut child,
            process_group,
            format!("could not inspect ready process group: {error}"),
        ),
    }
    let sent = signal_process_group(process_group, signal);
    let stopped = wait_for_group_quiescence(&mut child, process_group, Duration::from_secs(2));
    let (signal_attempt, status) = match (sent, stopped) {
        (Ok(output), Ok(Some(status))) => (output, status),
        (sent, stopped) => {
            terminate_and_panic(
                temp,
                &mut child,
                process_group,
                format!(
                    "signal {signal} did not stop the fixture cleanly; signal={}; wait={}",
                    describe_signal_result(sent),
                    describe_wait_result(stopped)
                ),
            );
        }
    };
    temp.mark_process_group_quiescent();
    let output = child
        .wait_with_output()
        .expect("collect signalled shell output");
    assert_eq!(
        (output.status.code(), output.status.signal()),
        (status.code(), status.signal()),
        "signalled shell status changed after quiescence"
    );
    let received = fs::read_to_string(&paths.signal_capture).expect("read received signal marker");
    assert_eq!(
        received, signal,
        "fixture did not record the requested signal (kill status {})",
        signal_attempt.status
    );
    output
}

fn wait_until_ready(child: &mut Child, ready: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if ready.is_file() {
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!(
                "shell exited before reaching signal point: {status}"
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err("shell did not reach signal point before timeout".to_owned())
}

fn signal_process_group(process_group: u32, signal: &str) -> io::Result<Output> {
    Command::new("/bin/kill")
        .arg("-s")
        .arg(signal)
        .arg(format!("-{process_group}"))
        .output()
}

fn wait_for_group_quiescence(
    child: &mut Child,
    process_group: u32,
    timeout: Duration,
) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let status = child.try_wait()?;
        if !process_group_is_alive(process_group)?
            && let Some(status) = status
        {
            return Ok(Some(status));
        }
        thread::sleep(Duration::from_millis(25));
    }
    Ok(None)
}

fn process_group_is_alive(process_group: u32) -> io::Result<bool> {
    let output = Command::new("ps").args(["-axo", "pgid=,stat="]).output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            "could not inspect signal-test process group",
        ));
    }
    let listing = String::from_utf8(output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    for line in listing.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let group = fields
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process group"))?
            .parse::<u32>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        let state = fields
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process state"))?;
        if group == process_group && !state.starts_with('Z') {
            return Ok(true);
        }
    }
    Ok(false)
}

struct GroupCleanup {
    quiescent: bool,
    detail: String,
}

fn terminate_process_group(child: &mut Child, process_group: u32) -> GroupCleanup {
    let mut diagnostics = Vec::new();
    for signal in ["TERM", "KILL"] {
        match signal_process_group(process_group, signal) {
            Ok(output) if output.status.success() => {}
            Ok(output) => diagnostics.push(format!(
                "{signal} returned {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) => diagnostics.push(format!("{signal} failed: {error}")),
        }
        match wait_for_group_quiescence(child, process_group, Duration::from_secs(2)) {
            Ok(Some(status)) => {
                return GroupCleanup {
                    quiescent: true,
                    detail: format!("quiescent after {signal} ({status})"),
                };
            }
            Ok(None) => diagnostics.push(format!("process group remained live after {signal}")),
            Err(error) => diagnostics.push(format!("could not verify after {signal}: {error}")),
        }
    }
    match child.kill() {
        Ok(()) => diagnostics.push("direct child kill requested".to_owned()),
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            diagnostics.push("direct child had already exited".to_owned());
        }
        Err(error) => diagnostics.push(format!("direct child kill failed: {error}")),
    }
    match signal_process_group(process_group, "KILL") {
        Ok(output) if output.status.success() => {}
        Ok(output) => diagnostics.push(format!("final KILL returned {}", output.status)),
        Err(error) => diagnostics.push(format!("final KILL failed: {error}")),
    }
    match wait_for_group_quiescence(child, process_group, Duration::from_secs(2)) {
        Ok(Some(status)) => GroupCleanup {
            quiescent: true,
            detail: format!("quiescent after final kill ({status}); {diagnostics:?}"),
        },
        Ok(None) => GroupCleanup {
            quiescent: false,
            detail: format!("could not establish process-group quiescence; {diagnostics:?}"),
        },
        Err(error) => GroupCleanup {
            quiescent: false,
            detail: format!("quiescence probe failed ({error}); {diagnostics:?}"),
        },
    }
}

fn terminate_and_panic(
    temp: &mut OwnedTempDir,
    child: &mut Child,
    process_group: u32,
    reason: impl std::fmt::Display,
) -> ! {
    let cleanup = terminate_process_group(child, process_group);
    if cleanup.quiescent {
        temp.mark_process_group_quiescent();
    }
    panic!("{reason}; cleanup={}", cleanup.detail);
}

fn describe_signal_result(result: io::Result<Output>) -> String {
    match result {
        Ok(output) => format!(
            "{} ({})",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(error) => error.to_string(),
    }
}

fn describe_wait_result(result: io::Result<Option<ExitStatus>>) -> String {
    match result {
        Ok(Some(status)) => format!("exited ({status})"),
        Ok(None) => "timed out".to_owned(),
        Err(error) => error.to_string(),
    }
}
