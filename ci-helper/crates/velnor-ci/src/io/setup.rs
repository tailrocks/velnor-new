// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Mutex;
use std::thread;

use velnor_gen::io::read_files;
use velnor_rust::{mise_files, tool_pins};

use super::{append_outputs, append_to, current_dir};
use crate::cli::RunArgs;
use crate::error::CiError;
use crate::setup::{check_tools, env_lines, output_lines, parse_reported};
use crate::tests_run::{SummaryTap, output_line, runs_nextest};

const ENV_VARIABLE: &str = "GITHUB_ENV";
const TEMP_VARIABLE: &str = "RUNNER_TEMP";
const LOG_FILE: &str = "velnor-run.log";
const NOT_FOUND: u8 = 127;
const NOT_RUNNABLE: u8 = 126;
const SIGNAL_BASE: i32 = 128;
const CHUNK: usize = 8192;

pub(super) fn prepare() -> Result<String, CiError> {
    let root = current_dir()?;
    let files = read_files(&root, &mise_files()).map_err(|e| CiError::new(e.to_string()))?;
    let pins = tool_pins(&files).map_err(|e| CiError::new(e.to_string()))?;
    let output = Command::new("mise")
        .args(["ls", "--current", "--json"])
        .current_dir(&root)
        .output()
        .map_err(|e| CiError::new(format!("cannot run mise: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "mise ls failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    check_tools(
        &pins,
        &parse_reported(&String::from_utf8_lossy(&output.stdout))?,
    )?;
    append_to(ENV_VARIABLE, &env_lines())?;
    append_outputs(&output_lines())?;
    Ok(String::new())
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
    let log = Mutex::new(log);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let counts = thread::scope(|scope| {
        let readers = [
            stdout.map(|from| scope.spawn(|| pump(from, std::io::stdout(), &log))),
            stderr.map(|from| scope.spawn(|| pump(from, std::io::stderr(), &log))),
        ];
        readers
            .into_iter()
            .flatten()
            .map(|reader| reader.join().ok().flatten())
            .collect::<Vec<_>>()
    });
    let status = child
        .wait()
        .map_err(|e| (2, format!("cannot wait for {}: {e}", args.command[0])))?;
    let code = exit_code(status);
    if runs_nextest(&args.command) {
        if let Some(tests_run) = counts.into_iter().flatten().next_back() {
            append_outputs(&output_line(tests_run))
                .map_err(|e| (2, format!("cannot write the step output: {e}")))?;
        }
    }
    let mut log = log
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    writeln!(log, "velnor-ci run: exit {code}").map_err(write_error)?;
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

fn pump(mut from: impl Read, mut to: impl Write, log: &Mutex<std::fs::File>) -> Option<u64> {
    let mut tap = SummaryTap::default();
    let mut buffer = [0_u8; CHUNK];
    while let Ok(read) = from.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        tap.feed(chunk);
        to.write_all(chunk).and_then(|()| to.flush()).ok();
        if let Ok(mut file) = log.lock() {
            file.write_all(chunk).ok();
        }
    }
    tap.finish()
}
