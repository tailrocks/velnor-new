// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]

include!("../src/entry.rs");

fn main() -> ExitCode {
    emit_plan_diagnostics();
    run_entry()
}

fn emit_plan_diagnostics() {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process;

    let mut args = std::env::args_os();
    if args.len() != 2 || args.nth(1).as_deref() != Some(OsStr::new("plan")) {
        return;
    }

    let Ok(mode) = std::env::var("VELNOR_PLAN_DIAGNOSTICS_MODE") else {
        return;
    };
    if !matches!(
        mode.as_str(),
        "control" | "fixture-warning" | "dogfood-warning"
    ) {
        return;
    }

    let Some(root) = std::env::current_dir()
        .ok()
        .and_then(|path| path.canonicalize().ok())
    else {
        return;
    };
    let temp_dir = std::env::var_os("TMPDIR").unwrap_or_else(|| "/tmp".into());
    eprintln!("Repository: {}", root.display());
    eprintln!(
        "Preview: {}/diagnostic-preview-{}",
        Path::new(&temp_dir).display(),
        process::id()
    );

    if mode == "fixture-warning" && root.file_name() == Some(OsStr::new("nested")) {
        eprintln!("warning: fixture-plan-diagnostic-regression");
    }
    if mode == "dogfood-warning"
        && std::env::var_os("VELNOR_PLAN_DIAGNOSTICS_ROOT").as_deref() == Some(root.as_os_str())
    {
        eprintln!("warning: dogfood-plan-diagnostic-regression");
    }
}
