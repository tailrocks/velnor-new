// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::cli::RunArgs;
use crate::stream::pump;

const TEMP_VARIABLE: &str = "RUNNER_TEMP";
const LOG_FILE: &str = "velnor-run.log";
const NOT_FOUND: u8 = 127;
const NOT_RUNNABLE: u8 = 126;
const SIGNAL_BASE: i32 = 128;
const PUMP_GRACE: Duration = Duration::from_secs(2);

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
    let write_error = |e: std::io::Error| (2, format!("cannot write {}: {e}", path.display()));
    writeln!(
        log,
        "velnor-ci run: {} ({} arguments)",
        args.command[0],
        args.command.len() - 1
    )
    .map_err(write_error)?;
    let mut signals = Signals::new([SIGINT, SIGTERM])
        .map_err(|e| (2, format!("cannot watch for SIGINT and SIGTERM: {e}")))?;
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
    let log = Arc::new(Mutex::new(log));
    let (sender, receiver) = mpsc::channel();
    let mut streams = 0;
    if let Some(from) = child.stdout.take() {
        streams += 1;
        spawn_pump(from, std::io::stdout(), &log, sender.clone());
    }
    if let Some(from) = child.stderr.take() {
        streams += 1;
        spawn_pump(from, std::io::stderr(), &log, sender.clone());
    }
    drop(sender);
    let handle = signals.handle();
    thread::spawn(move || {
        for received in signals.forever() {
            if let (Some(pid), Ok(signal)) = (pid, Signal::try_from(received)) {
                kill(pid, signal).ok();
            }
        }
    });
    let status = child.wait();
    handle.close();
    let status = status.map_err(|e| (2, format!("cannot wait for {}: {e}", args.command[0])))?;
    let code = exit_code(status);
    let mut lost = None;
    for _ in 0..streams {
        match receiver.recv_timeout(PUMP_GRACE) {
            Ok(failure) => lost = lost.or(failure),
            Err(_) => break,
        }
    }
    let mut log = log
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Err(error) = writeln!(log, "velnor-ci run: exit {code}") {
        lost = lost.or(Some(error));
    }
    match lost {
        Some(error) if code == 0 => Err(write_error(error)),
        Some(error) => {
            eprintln!("velnor-ci: cannot write {}: {error}", path.display());
            Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
        }
        None => Ok(ExitCode::from(u8::try_from(code).unwrap_or(1))),
    }
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

fn spawn_pump(
    from: impl Read + Send + 'static,
    to: impl Write + Send + 'static,
    log: &Arc<Mutex<std::fs::File>>,
    done: mpsc::Sender<Option<std::io::Error>>,
) {
    let log = Arc::clone(log);
    thread::spawn(move || {
        done.send(pump(from, to, &log)).ok();
    });
}
