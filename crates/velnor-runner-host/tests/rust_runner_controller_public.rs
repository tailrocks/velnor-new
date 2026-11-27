// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "rust_runner_controller_public/admission.rs"]
mod admission;
#[path = "rust_runner_controller_public/checks.rs"]
mod checks;
#[path = "rust_runner_controller_public/child.rs"]
mod child;
#[path = "rust_runner_controller_public/credentials.rs"]
mod credentials;
#[path = "rust_runner_controller_public/deadline.rs"]
mod deadline;
#[path = "rust_runner_controller_public/events.rs"]
mod events;
#[path = "rust_runner_controller_public/failure.rs"]
mod failure;
#[path = "rust_runner_controller_public/faults.rs"]
mod faults;
#[path = "rust_runner_controller_public/fixtures.rs"]
mod fixtures;
#[path = "rust_runner_controller_public/harness.rs"]
mod harness;
#[path = "rust_runner_controller_public/heartbeat.rs"]
mod heartbeat;
#[path = "rust_runner_controller_public/limit.rs"]
mod limit;
#[path = "rust_runner_controller_public/once.rs"]
mod once;
#[path = "rust_runner_controller_public/organization.rs"]
mod organization;
#[path = "rust_runner_controller_public/runners.rs"]
mod runners;
#[path = "rust_runner_controller_public/shutdown.rs"]
mod shutdown;
#[path = "rust_runner_controller_public/slot.rs"]
mod slot;
#[path = "rust_runner_controller_public/source.rs"]
mod source;
#[path = "rust_runner_controller_public/startup.rs"]
mod startup;
#[path = "rust_runner_controller_public/sweep.rs"]
mod sweep;
#[path = "rust_runner_controller_public/unavailable.rs"]
mod unavailable;

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
fn rust_runner_controller_proves_cleanup_after_a_failed_launch() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => failure::run("rust_runner_controller_proves_cleanup_after_a_failed_launch"),
    }
}

#[test]
fn rust_runner_controller_defers_a_check_github_could_not_answer() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => unavailable::run("rust_runner_controller_defers_a_check_github_could_not_answer"),
    }
}

#[test]
fn rust_runner_controller_degrades_after_three_failed_runner_exits() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => runners::run("rust_runner_controller_degrades_after_three_failed_runner_exits"),
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

#[test]
fn rust_runner_controller_reports_stale_heartbeats_as_degraded() -> TestResult<()> {
    heartbeat::run()
}

#[test]
fn rust_runner_controller_adopts_only_its_own_organization_scale_set() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => {
            organization::run("rust_runner_controller_adopts_only_its_own_organization_scale_set")
        }
    }
}

#[test]
fn rust_runner_controller_leaves_a_held_startup_when_host_toml_changes() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => startup::run("rust_runner_controller_leaves_a_held_startup_when_host_toml_changes"),
    }
}

#[test]
fn rust_runner_controller_reports_a_failing_session_refresh_as_degraded() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => faults::degraded_session(
            "rust_runner_controller_reports_a_failing_session_refresh_as_degraded",
        ),
    }
}

#[test]
fn rust_runner_controller_cleans_up_after_a_controller_error() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => faults::cleans_up_after_an_error(
            "rust_runner_controller_cleans_up_after_a_controller_error",
        ),
    }
}

#[test]
fn rust_runner_controller_does_no_cleanup_after_losing_the_daemon_lock() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => faults::leaves_everything_after_a_lost_lock(
            "rust_runner_controller_does_no_cleanup_after_losing_the_daemon_lock",
        ),
    }
}

#[test]
fn rust_runner_controller_reads_host_toml_up_to_the_shared_limit() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => limit::run("rust_runner_controller_reads_host_toml_up_to_the_shared_limit"),
    }
}

#[test]
fn rust_runner_controller_ends_an_orb_call_that_hangs() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => deadline::run("rust_runner_controller_ends_an_orb_call_that_hangs"),
    }
}
