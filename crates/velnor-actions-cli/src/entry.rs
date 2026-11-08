// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "generator")]
#[path = "../src/args.rs"]
mod args;
#[path = "../src/dispatch/compiler.rs"]
mod compiler;
#[cfg(feature = "generator")]
#[path = "../src/dispatch.rs"]
mod dispatch;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_config.rs"]
mod dispatch_config;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_deps.rs"]
mod dispatch_deps;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_generate.rs"]
mod dispatch_generate;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_homebrew_adoption.rs"]
mod dispatch_homebrew_adoption;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_image_maintenance.rs"]
mod dispatch_image_maintenance;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_local_release.rs"]
mod dispatch_local_release;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_qualification.rs"]
mod dispatch_qualification;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_qualify_tools.rs"]
mod dispatch_qualify_tools;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_repo_policy.rs"]
mod dispatch_repo_policy;
#[cfg(feature = "generator")]
#[path = "../src/dispatch_unmanaged.rs"]
mod dispatch_unmanaged;
#[path = "../src/ledger_helper.rs"]
mod ledger_helper;

use std::process::ExitCode;

fn run_entry() -> ExitCode {
    #[cfg(feature = "generator")]
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--cargo-plugin")) {
        return if velnor_actions_orchestrator::crate_release_credential_provider::run().is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    if let Some(outcome) = compiler::try_observer_entry() {
        return outcome;
    }
    if let Some(outcome) = ledger_helper::try_entry() {
        return outcome;
    }
    run_generator()
}

#[cfg(feature = "generator")]
fn run_generator() -> ExitCode {
    if std::env::args_os().len() <= 1
        && let Some(outcome) = dispatch::try_internal()
    {
        return outcome;
    }
    dispatch::run_public()
}

#[cfg(not(feature = "generator"))]
fn run_generator() -> ExitCode {
    ledger_helper::reject()
}
