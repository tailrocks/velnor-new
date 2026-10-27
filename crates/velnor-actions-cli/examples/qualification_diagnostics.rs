// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]

include!("../src/entry.rs");

fn main() -> ExitCode {
    let outcome = run_entry();
    if outcome != ExitCode::SUCCESS || !is_plan_command() {
        return outcome;
    }

    match emit_plan_diagnostics() {
        Some(diagnostic_outcome) => diagnostic_outcome,
        None => outcome,
    }
}

fn is_plan_command() -> bool {
    use std::ffi::OsStr;

    let mut args = std::env::args_os();
    args.len() == 2 && args.nth(1).as_deref() == Some(OsStr::new("plan"))
}

fn emit_plan_diagnostics() -> Option<ExitCode> {
    use std::ffi::OsStr;
    use std::path::Path;

    let mode = std::env::var("VELNOR_PLAN_DIAGNOSTICS_MODE").ok()?;
    if mode == "quiet" {
        return Some(ExitCode::SUCCESS);
    }
    if !matches!(
        mode.as_str(),
        "control" | "warning-all" | "nested-only" | "dogfood-only" | "fail57"
    ) {
        return Some(ExitCode::from(56));
    }

    let Some(root) = std::env::current_dir()
        .ok()
        .and_then(|path| path.canonicalize().ok())
    else {
        return Some(ExitCode::from(56));
    };
    let Some(source_root) = std::env::var_os("VELNOR_PLAN_DIAGNOSTICS_ROOT")
        .and_then(|path| Path::new(&path).canonicalize().ok())
    else {
        return Some(ExitCode::from(56));
    };
    let is_nested = root.file_name() == Some(OsStr::new("nested"));
    let is_dogfood = root == source_root;

    eprintln!("Repository: {}", root.display());
    eprintln!("Preview: {}", root.join("diagnostic-preview").display());
    eprintln!(
        "Plan context: repository={} source-root={}",
        root.display(),
        source_root.display()
    );

    let emit_warning = match mode.as_str() {
        "warning-all" => true,
        "nested-only" | "fail57" => is_nested,
        "dogfood-only" => is_dogfood,
        _ => false,
    };
    if emit_warning {
        eprintln!("warning: plan-diagnostic-regression");
    }
    if mode == "fail57" && is_nested {
        Some(ExitCode::from(57))
    } else {
        Some(ExitCode::SUCCESS)
    }
}
