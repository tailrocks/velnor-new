// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use super::append_outputs;
use crate::cli::RunArgs;
use crate::setup::DoctestCount;
use crate::stream::{Pumped, pump_counted, separate_group};
use crate::tests_run::{TestCounts, output_lines, runs_nextest};

const TEMP_VARIABLE: &str = "RUNNER_TEMP";
const LOG_FILE: &str = "velnor-run.log";
const NOT_FOUND: u8 = 127;
const NOT_RUNNABLE: u8 = 126;
const SIGNAL_BASE: i32 = 128;
const PUMP_GRACE: Duration = Duration::from_secs(2);
const TERMINAL_VARIABLE: &str = "VELNOR_CI_RUN_TERMINAL";
const CONTROLLING_TERMINAL: &str = "/dev/tty";
const PUMP_GRACE_VARIABLE: &str = "VELNOR_CI_RUN_PUMP_GRACE_MS";

fn pump_grace() -> Duration {
    std::env::var(PUMP_GRACE_VARIABLE)
        .ok()
        .and_then(|millis| millis.parse().ok())
        .map_or(PUMP_GRACE, Duration::from_millis)
}

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
    let signals = Signals::new([SIGINT, SIGTERM])
        .map_err(|e| (2, format!("cannot watch for SIGINT and SIGTERM: {e}")))?;
    let (mut child, own_group) = spawn_child(&args.command)?;
    let pid = i32::try_from(child.id()).map(Pid::from_raw).ok();
    let log = Arc::new(Mutex::new(log));
    let doctests = Arc::new(Mutex::new(DoctestCount::default()));
    let (sender, receiver) = mpsc::channel();
    let mut streams = 0;
    if let Some(from) = child.stdout.take() {
        streams += 1;
        let counted = args.require_doctests.then(|| Arc::clone(&doctests));
        spawn_pump(from, std::io::stdout(), &log, counted, sender.clone());
    }
    if let Some(from) = child.stderr.take() {
        streams += 1;
        spawn_pump(from, std::io::stderr(), &log, None, sender.clone());
    }
    drop(sender);
    let handle = signals.handle();
    forward(signals, pid, own_group);
    let status = child.wait();
    handle.close();
    let status = status.map_err(|e| (2, format!("cannot wait for {}: {e}", args.command[0])))?;
    let mut code = exit_code(status);
    let (mut lost, counts) = collect(&receiver, streams);
    if runs_nextest(&args.command)
        && let Some(counts) = counts
    {
        append_outputs(&output_lines(counts))
            .map_err(|e| (2, format!("cannot write the step output: {e}")))?;
    }
    let ran = doctests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish();
    if code == 0 && args.require_doctests && ran == 0 {
        eprintln!("velnor-ci: the command ran 0 doctests, so this step checked nothing");
        code = 1;
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

fn has_terminal() -> bool {
    match std::env::var(TERMINAL_VARIABLE).as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => OpenOptions::new()
            .read(true)
            .open(CONTROLLING_TERMINAL)
            .is_ok(),
    }
}

fn spawn_child(argv: &[String]) -> Result<(std::process::Child, bool), (u8, String)> {
    let own_group = separate_group(has_terminal());
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if own_group {
        command.process_group(0);
    }
    let child = command.spawn().map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            NOT_FOUND
        } else {
            NOT_RUNNABLE
        };
        (code, format!("cannot run {}: {e}", argv[0]))
    })?;
    Ok((child, own_group))
}

fn forward(mut signals: Signals, pid: Option<Pid>, own_group: bool) {
    thread::spawn(move || {
        for received in signals.forever() {
            if let (Some(pid), Ok(signal)) = (pid, Signal::try_from(received)) {
                if own_group {
                    killpg(pid, signal).ok();
                } else {
                    kill(pid, signal).ok();
                }
            }
        }
    });
}

fn collect(
    receiver: &mpsc::Receiver<Pumped>,
    streams: usize,
) -> (Option<std::io::Error>, Option<TestCounts>) {
    let mut lost = None;
    let mut counts = None;
    let deadline = Instant::now() + pump_grace();
    for _ in 0..streams {
        match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(pumped) => {
                lost = lost.or(pumped.failure);
                counts = pumped.counts.or(counts);
            }
            Err(_) => break,
        }
    }
    (lost, counts)
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
    count: Option<Arc<Mutex<DoctestCount>>>,
    done: mpsc::Sender<Pumped>,
) {
    let log = Arc::clone(log);
    thread::spawn(move || {
        done.send(pump_counted(from, to, &log, count.as_deref()))
            .ok();
    });
}
