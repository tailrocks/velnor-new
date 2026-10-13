//! Run all synchronous readiness probes inside one killable child.

use std::ffi::OsStr;
use std::io::Write;
use std::path::PathBuf;
use std::process::{ExitCode, ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::time::{Instant, timeout_at};
use velnor_runner_host::{Readiness, controller_readiness};

const INTERNAL_FLAG: &str = "--internal-readiness-scan";
const MAX_STATUS_BYTES: usize = 64;
const MAX_STATUS_READ: u64 = 65;
const REAP_RESERVE: Duration = Duration::from_millis(300);

enum ChildWait {
    Exited(ExitStatus),
    TimedOut,
    Failed,
}

/// Run the private child operation when its exact argument shape is present.
pub(crate) fn run_internal() -> Option<ExitCode> {
    let mut args = std::env::args_os();
    let _program = args.next()?;
    if args.next()?.as_os_str() != OsStr::new(INTERNAL_FLAG) {
        return None;
    }
    let Some(state) = args.next() else {
        return Some(ExitCode::from(2));
    };
    if args.next().is_some() {
        return Some(ExitCode::from(2));
    }
    Some(write_readiness(&PathBuf::from(state)))
}

/// Assess readiness in a child so blocking system calls can be killed.
pub(crate) async fn check(state: PathBuf, deadline: Instant) -> Readiness {
    let now = Instant::now();
    let probe_deadline = deadline.checked_sub(REAP_RESERVE).unwrap_or(now);
    if probe_deadline <= now {
        return Readiness::Degraded;
    }
    let Ok(mut command) = readiness_command(state) else {
        return Readiness::Degraded;
    };
    let Ok(mut child) = command.spawn() else {
        return Readiness::Degraded;
    };
    match bounded_wait(&mut child, probe_deadline, deadline).await {
        ChildWait::Exited(status) if status.success() => read_status(&mut child, deadline).await,
        ChildWait::Exited(_) | ChildWait::TimedOut | ChildWait::Failed => Readiness::Degraded,
    }
}

fn readiness_command(state: PathBuf) -> std::io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .env_clear()
        .arg(INTERNAL_FLAG)
        .arg(state)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    Ok(command)
}

async fn bounded_wait(
    child: &mut Child,
    probe_deadline: Instant,
    reap_deadline: Instant,
) -> ChildWait {
    match timeout_at(probe_deadline, child.wait()).await {
        Ok(Ok(status)) => ChildWait::Exited(status),
        Ok(Err(_)) => child_failure(child, reap_deadline).await,
        Err(_) => {
            if stop_and_reap(child, reap_deadline).await {
                ChildWait::TimedOut
            } else {
                ChildWait::Failed
            }
        }
    }
}

async fn child_failure(child: &mut Child, deadline: Instant) -> ChildWait {
    if !stop_and_reap(child, deadline).await {
        eprintln!("readiness child could not be reaped after wait failure");
    }
    ChildWait::Failed
}

async fn stop_and_reap(child: &mut Child, deadline: Instant) -> bool {
    let kill = child.start_kill();
    if let Err(error) = &kill
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        eprintln!("readiness child kill failed: {error}");
    }
    match timeout_at(deadline, child.wait()).await {
        Ok(Ok(_)) => true,
        Ok(Err(error)) => {
            eprintln!("readiness child wait failed: {error}");
            false
        }
        Err(_) => {
            eprintln!("readiness child was not reaped before the deadline");
            false
        }
    }
}

async fn read_status(child: &mut Child, deadline: Instant) -> Readiness {
    let Some(stdout) = child.stdout.take() else {
        return Readiness::Degraded;
    };
    let mut bytes = Vec::new();
    let mut output = stdout.take(MAX_STATUS_READ);
    let read = timeout_at(deadline, output.read_to_end(&mut bytes)).await;
    if !matches!(read, Ok(Ok(_))) || bytes.len() > MAX_STATUS_BYTES {
        return Readiness::Degraded;
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Readiness::Degraded;
    };
    // A `ready` token is rejected because this partial observer cannot prove full reconciliation.
    match text.strip_suffix('\n') {
        Some("waiting_for_engine") => Readiness::WaitingForEngine,
        Some("waiting_for_credentials") => Readiness::WaitingForCredentials,
        Some("reconciling") => Readiness::Reconciling,
        Some("draining") => Readiness::Draining,
        _ => Readiness::Degraded,
    }
}

fn write_readiness(state: &std::path::Path) -> ExitCode {
    let readiness = controller_readiness(state);
    let mut output = std::io::stdout().lock();
    if output.write_all(readiness.as_str().as_bytes()).is_err()
        || output.write_all(b"\n").is_err()
        || output.flush().is_err()
    {
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
