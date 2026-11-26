// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "rust_runner_controller_public/admission.rs"]
mod admission;
#[path = "rust_runner_controller_public/checks.rs"]
mod checks;
#[path = "rust_runner_controller_public/child.rs"]
mod child;
#[path = "rust_runner_controller_public/fixtures.rs"]
mod fixtures;
#[path = "rust_runner_controller_public/harness.rs"]
mod harness;
#[path = "rust_runner_controller_public/once.rs"]
mod once;
#[path = "rust_runner_controller_public/shutdown.rs"]
mod shutdown;
#[path = "rust_runner_controller_public/slot.rs"]
mod slot;
#[path = "rust_runner_controller_public/source.rs"]
mod source;
#[path = "rust_runner_controller_public/sweep.rs"]
mod sweep;

use fixtures::TestResult;

fn child_root() -> Option<std::path::PathBuf> {
    std::env::var_os(harness::CHILD_MARKER).map(Into::into)
}

#[test]
fn rust_runner_controller_runs_authorized_job_once() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => once::run("rust_runner_controller_runs_authorized_job_once"),
    }
}

#[test]
fn rust_runner_controller_never_acquires_unapproved_source() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => source::run("rust_runner_controller_never_acquires_unapproved_source"),
    }
}

#[test]
fn rust_runner_controller_holds_slot_until_cleanup_proven() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => slot::run("rust_runner_controller_holds_slot_until_cleanup_proven"),
    }
}

#[test]
fn rust_runner_controller_sweeps_machine_before_admission() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => sweep::run("rust_runner_controller_sweeps_machine_before_admission"),
    }
}

#[test]
fn rust_runner_controller_stops_jobs_on_shutdown() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => shutdown::run("rust_runner_controller_stops_jobs_on_shutdown"),
    }
}

#[test]
fn rust_runner_controller_closes_admission_on_drain_and_config_change() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => {
            admission::run("rust_runner_controller_closes_admission_on_drain_and_config_change")
        }
    }
}
