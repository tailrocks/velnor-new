// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Mutex;
use std::thread;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use signal_hook::consts::SIGTERM;
use signal_hook::iterator::Signals;

use crate::cli::RunArgs;
use crate::stream::pump;

const TEMP_VARIABLE: &str = "RUNNER_TEMP";
const LOG_FILE: &str = "velnor-run.log";
const NOT_FOUND: u8 = 127;
const NOT_RUNNABLE: u8 = 126;
const SIGNAL_BASE: i32 = 128;

fn log_path() -> Result<PathBuf, (u8, String)> {
    std::env::var_os(TEMP_VARIABLE)
        .filter(|dir| !dir.is_empty())
        .map(|dir| PathBuf::from(dir).join(LOG_FILE))
        .ok_or_else(|| (2, format!("{TEMP_VARIABLE} is not set")))
}

fn open_log(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

pub(super) fn run(args: &RunArgs) -> ExitCode {
    match execute(args) {
        Ok(code) => code,
        Err((code, message)) => {
            eprintln!("velnor-ci: {message}");
            ExitCode::from(code)
        }
    }
}

fn execute(args: &RunArgs) -> Result<ExitCode, (u8, String)> {
    let path = log_path()?;
    let mut log =
        open_log(&path).map_err(|e| (2, format!("cannot open {}: {e}", path.display())))?;
    writeln!(
        log,
        "velnor-ci run: {} ({} arguments)",
        args.command[0],
        args.command.len() - 1
    )
    .map_err(|e| (2, format!("cannot write {}: {e}", path.display())))?;
    let mut signals =
        Signals::new([SIGTERM]).map_err(|e| (2, format!("cannot watch for SIGTERM: {e}")))?;
    let mut child = Command::new(&args.command[0])
        .args(&args.command[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                NOT_FOUND
            } else {
                NOT_RUNNABLE
            };
            (code, format!("cannot run {}: {e}", args.command[0]))
        })?;
    let pid = i32::try_from(child.id()).map(Pid::from_raw).ok();
    let log = Mutex::new(log);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let handle = signals.handle();
    let status = thread::scope(|scope| {
        if let Some(from) = stdout {
            scope.spawn(|| pump(from, std::io::stdout(), &log));
        }
        if let Some(from) = stderr {
            scope.spawn(|| pump(from, std::io::stderr(), &log));
        }
        scope.spawn(|| {
            for _ in signals.forever() {
                if let Some(pid) = pid {
                    kill(pid, Signal::SIGTERM).ok();
                }
            }
        });
        let status = child.wait();
        handle.close();
        status
    });
    let status = status.map_err(|e| (2, format!("cannot wait for {}: {e}", args.command[0])))?;
    let code = exit_code(status);
    let mut log = log
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Err(error) = writeln!(log, "velnor-ci run: exit {code}") {
        eprintln!("velnor-ci: cannot write {}: {error}", path.display());
    }
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

#[cfg(unix)]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|signal| SIGNAL_BASE + signal))
        .unwrap_or(1)
}

#[cfg(not(unix))]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}
