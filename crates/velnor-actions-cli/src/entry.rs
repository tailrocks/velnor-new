// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/args.rs"]
mod args;
#[path = "../src/dispatch/compiler.rs"]
mod compiler;
#[path = "../src/dispatch.rs"]
mod dispatch;
#[path = "../src/dispatch_config.rs"]
mod dispatch_config;
#[path = "../src/dispatch_deps.rs"]
mod dispatch_deps;
#[path = "../src/dispatch_generate.rs"]
mod dispatch_generate;
#[path = "../src/dispatch_homebrew_adoption.rs"]
mod dispatch_homebrew_adoption;
#[path = "../src/dispatch_image_maintenance.rs"]
mod dispatch_image_maintenance;
#[path = "../src/dispatch_local_release.rs"]
mod dispatch_local_release;
#[path = "../src/dispatch_qualification.rs"]
mod dispatch_qualification;
#[path = "../src/dispatch_qualify_tools.rs"]
mod dispatch_qualify_tools;
#[path = "../src/dispatch_repo_policy.rs"]
mod dispatch_repo_policy;
#[path = "../src/dispatch_unmanaged.rs"]
mod dispatch_unmanaged;

use std::process::ExitCode;

fn run_entry() -> ExitCode {
    if let Some(outcome) = compiler::try_observer_entry() {
        return outcome;
    }
    if std::env::args_os().len() <= 1
        && let Some(outcome) = dispatch::try_internal()
    {
        return outcome;
    }
    dispatch::run_public()
}
