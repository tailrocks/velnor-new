// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::sync::Mutex;
use std::thread;

use velnor_gen::io::read_files;
use velnor_rust::{TOOLCHAIN_PLAIN, TOOLCHAIN_TOML, mise_files, tool_pins, toolchain};

use super::{append_outputs, append_to, current_dir};
use crate::cli::RunArgs;
use crate::error::CiError;
use crate::setup::{check_toolchain, check_tools, env_lines, output_lines, parse_reported};

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
    let declared = read_files(&root, &[TOOLCHAIN_PLAIN, TOOLCHAIN_TOML])
        .map_err(|e| CiError::new(e.to_string()))?;
    if let Some(declared) = toolchain(&declared).map_err(|e| CiError::new(e.to_string()))? {
        let host = host_triple(&root)?;
        check_toolchain(
            &declared,
            &host,
            &tool_output(&root, "rustup", &["component", "list"])?,
            &tool_output(&root, "rustup", &["target", "list", "--installed"])?,
        )?;
    }
    append_to(ENV_VARIABLE, &env_lines())?;
    append_outputs(&output_lines())?;
    Ok(String::new())
}

fn tool_output(root: &std::path::Path, program: &str, args: &[&str]) -> Result<String, CiError> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| CiError::new(format!("cannot run {program}: {e}")))?;
    if !output.status.success() {
        return Err(CiError::new(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn host_triple(root: &std::path::Path) -> Result<String, CiError> {
    tool_output(root, "rustc", &["-vV"])?
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| CiError::new("rustc -vV does not report a host".to_owned()))
}

fn log_path() -> PathBuf {
    std::env::var_os(TEMP_VARIABLE)
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join(LOG_FILE)
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
    let path = log_path();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| (2, format!("cannot open {}: {e}", path.display())))?;
    let text = args.command.join(" ");
    let write_error = |e: std::io::Error| (2, format!("cannot write {}: {e}", path.display()));
    writeln!(log, "velnor-ci run: {text}").map_err(write_error)?;
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
    thread::scope(|scope| {
        if let Some(from) = stdout {
            scope.spawn(|| pump(from, std::io::stdout(), &log));
        }
        if let Some(from) = stderr {
            scope.spawn(|| pump(from, std::io::stderr(), &log));
        }
    });
    let status = child
        .wait()
        .map_err(|e| (2, format!("cannot wait for {}: {e}", args.command[0])))?;
    let code = exit_code(status);
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

fn pump(mut from: impl Read, mut to: impl Write, log: &Mutex<std::fs::File>) {
    let mut buffer = [0_u8; CHUNK];
    while let Ok(read) = from.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        to.write_all(chunk).and_then(|()| to.flush()).ok();
        if let Ok(mut file) = log.lock() {
            file.write_all(chunk).ok();
        }
    }
}
