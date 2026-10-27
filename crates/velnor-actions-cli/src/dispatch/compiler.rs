// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const RUSTC_ENTRY: &str = "rustc";
const RUSTDOC_ENTRY: &str = "rustdoc";
const CLIPPY_DRIVER_ENTRY: &str = "clippy-driver";
const CC_ENTRY: &str = "cc";
const CXX_ENTRY: &str = "cxx";

const PREVIOUS_RUSTC_WRAPPER_ENV: &str = "MBX_PREVIOUS_RUSTC_WRAPPER";
const PREVIOUS_RUSTDOC_ENV: &str = "MBX_PREVIOUS_RUSTDOC";
const PREVIOUS_CC_ENV: &str = "VELNOR_PREVIOUS_CC";
const PREVIOUS_CXX_ENV: &str = "VELNOR_PREVIOUS_CXX";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObserverEntry {
    Rustc,
    Rustdoc,
    Cc,
    Cxx,
}

struct ResolvedEntry {
    program: OsString,
    args: Vec<OsString>,
}

pub(crate) fn try_observer_entry() -> Option<ExitCode> {
    let mut argv = env::args_os();
    let argv_zero = argv.next()?;
    let entry = entry_for_argv_zero(&argv_zero)?;
    let rest: Vec<OsString> = argv.collect();
    Some(run_entry(entry, &argv_zero, &rest))
}

fn entry_for_argv_zero(argv_zero: &OsStr) -> Option<ObserverEntry> {
    let base = Path::new(argv_zero)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let stem = base.strip_suffix(".exe").unwrap_or(base);
    match stem {
        RUSTC_ENTRY | CLIPPY_DRIVER_ENTRY => Some(ObserverEntry::Rustc),
        RUSTDOC_ENTRY => Some(ObserverEntry::Rustdoc),
        CC_ENTRY => Some(ObserverEntry::Cc),
        CXX_ENTRY => Some(ObserverEntry::Cxx),
        _ => None,
    }
}

fn run_entry(entry: ObserverEntry, argv_zero: &OsStr, rest: &[OsString]) -> ExitCode {
    let current_exe: PathBuf = env::current_exe().unwrap_or_else(|_| PathBuf::from(argv_zero));
    match resolve_entry(entry, rest, &current_exe) {
        Ok(resolved) => exec_resolved(&resolved),
        Err(problem) => {
            eprintln!("compiler_observer_rejected: {problem}");
            ExitCode::FAILURE
        }
    }
}

fn resolve_entry(
    entry: ObserverEntry,
    rest: &[OsString],
    current_exe: &Path,
) -> Result<ResolvedEntry, String> {
    match entry {
        ObserverEntry::Rustc => resolve_rustc(rest, current_exe),
        ObserverEntry::Rustdoc => resolve_with_previous(PREVIOUS_RUSTDOC_ENV, rest, current_exe),
        ObserverEntry::Cc => resolve_with_previous(PREVIOUS_CC_ENV, rest, current_exe),
        ObserverEntry::Cxx => resolve_with_previous(PREVIOUS_CXX_ENV, rest, current_exe),
    }
}

fn resolve_rustc(rest: &[OsString], current_exe: &Path) -> Result<ResolvedEntry, String> {
    let Some(real) = rest.first() else {
        return Err("compiler_real_missing".to_owned());
    };
    reject_nul("compiler_real", real)?;
    if real.is_empty() {
        return Err("compiler_real_missing".to_owned());
    }
    if is_self_executable(real, current_exe) {
        return Err("compiler_real_recursion".to_owned());
    }
    match env::var_os(PREVIOUS_RUSTC_WRAPPER_ENV) {
        Some(previous) => {
            if previous.is_empty() {
                return Err("compiler_previous_malformed".to_owned());
            }
            reject_nul("compiler_previous_wrapper", &previous)?;
            if is_self_executable(&previous, current_exe) {
                return Err("compiler_previous_recursion".to_owned());
            }
            Ok(ResolvedEntry {
                program: previous,
                args: rest.to_vec(),
            })
        }
        None => Ok(ResolvedEntry {
            program: real.clone(),
            args: rest[1..].to_vec(),
        }),
    }
}

fn resolve_with_previous(
    previous_env: &str,
    rest: &[OsString],
    current_exe: &Path,
) -> Result<ResolvedEntry, String> {
    let Some(previous) = env::var_os(previous_env) else {
        return Err("compiler_real_missing".to_owned());
    };
    let text = previous.to_string_lossy().into_owned();
    if text.is_empty() || text.contains('\0') {
        return Err("compiler_real_malformed".to_owned());
    }
    if !text.starts_with('/') {
        return Err("compiler_real_not_absolute".to_owned());
    }
    if is_self_executable(&previous, current_exe) {
        return Err("compiler_real_recursion".to_owned());
    }
    Ok(ResolvedEntry {
        program: previous,
        args: rest.to_vec(),
    })
}

fn reject_nul(field: &str, value: &OsStr) -> Result<(), String> {
    if value.to_string_lossy().contains('\0') {
        return Err(format!("{field}_malformed"));
    }
    Ok(())
}

fn is_self_executable(candidate: &OsStr, current_exe: &Path) -> bool {
    let candidate_path = Path::new(candidate);
    if candidate_path == current_exe {
        return true;
    }
    let Ok(candidate_full) = candidate_path.canonicalize() else {
        return false;
    };
    let Ok(own_full) = current_exe.canonicalize() else {
        return candidate_full == current_exe;
    };
    candidate_full == own_full
}

fn exec_resolved(resolved: &ResolvedEntry) -> ExitCode {
    match Command::new(&resolved.program)
        .args(&resolved.args)
        .status()
    {
        Ok(status) => match status.code() {
            Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            None => ExitCode::FAILURE,
        },
        Err(error) => {
            eprintln!(
                "compiler_observer_spawn_failed: {}",
                single_line(&error.to_string())
            );
            ExitCode::FAILURE
        }
    }
}

fn single_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}
