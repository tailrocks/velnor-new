// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    GenerateOptions, OperatorToolchainGuard, generate_dispatched, parse_dispatch_mode, prepare,
    prepare_with_operator_guard, resolve_root,
};

use crate::dispatch::{fail_public, working_dir};

#[path = "dispatch_generate_warnings.rs"]
mod warning_format;

pub(crate) fn run_generate(
    output_dir: Option<PathBuf>,
    mode: Option<String>,
    guard: Option<&OperatorToolchainGuard>,
) -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let options = GenerateOptions { output_dir };
    let root = match resolve_root(&cwd) {
        Ok(root) => root,
        Err(error) => return fail_public(&error),
    };
    let preparation = match match guard {
        Some(guard) => prepare_with_operator_guard(&root, guard),
        None => prepare(&root),
    } {
        Ok(preparation) => preparation,
        Err(error) => return fail_public(&error),
    };
    let dispatch = match mode {
        Some(text) => match parse_dispatch_mode(&text) {
            Ok(mode) => Some(mode),
            Err(error) => return fail_public(&error),
        },
        None => None,
    };
    match generate_dispatched(&preparation, &options, dispatch) {
        Ok(report) => {
            if preparation.discovery.consumer_manifest_stand_in {
                eprintln!(
                    "velnor-actions: WARNING: .velnor/release-manifest.json is absent; generated workflows use a debug-only stand-in that MUST NOT ship"
                );
            }
            if let Some(dir) = &options.output_dir {
                eprintln!("Preview: {}", absolute_preview(&cwd, dir).display());
                eprintln!("Repository: {}", root.display());
            }
            for path in &report.files_written {
                eprintln!("{path}");
            }
            for recommendation in &report.recommendations {
                eprintln!("{recommendation}");
            }
            for warning in warning_format::warning_lines(&report.warnings) {
                eprintln!("{warning}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}

fn absolute_preview(cwd: &Path, dir: &Path) -> PathBuf {
    let joined = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        cwd.join(dir)
    };
    joined.canonicalize().unwrap_or(joined)
}
