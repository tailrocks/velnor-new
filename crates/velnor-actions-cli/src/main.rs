// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
mod args;
mod dispatch;
mod dispatch_config;
mod dispatch_deps;
mod dispatch_generate;
mod dispatch_homebrew_adoption;
mod dispatch_image_maintenance;
mod dispatch_local_release;
mod dispatch_qualification;
mod dispatch_qualify_tools;
mod dispatch_repo_policy;
mod dispatch_unmanaged;

use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args_os().len() <= 1
        && let Some(outcome) = dispatch::try_internal()
    {
        return outcome;
    }
    dispatch::run_public()
}
