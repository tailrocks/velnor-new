// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "rust_runner_controller_public/acquire_shape.rs"]
mod acquire_shape;
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
#[path = "rust_runner_controller_public/scope.rs"]
mod scope;
#[path = "rust_runner_controller_public/shutdown.rs"]
mod shutdown;
#[path = "rust_runner_controller_public/signals.rs"]
mod signals;
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
fn rust_runner_controller_defers_a_rate_limited_check() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => unavailable::rate_limited("rust_runner_controller_defers_a_rate_limited_check"),
    }
}

#[test]
fn rust_runner_controller_refuses_a_403_without_rate_limit_signals() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => unavailable::refused_for_good(
            "rust_runner_controller_refuses_a_403_without_rate_limit_signals",
        ),
    }
}

#[test]
fn rust_runner_controller_degrades_after_three_failed_runner_exits() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => runners::counts_failures_not_breaches(
            "rust_runner_controller_degrades_after_three_failed_runner_exits",
        ),
    }
}

#[test]
fn rust_runner_controller_keeps_the_runner_health_across_a_config_reload() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => runners::keeps_the_health_across_a_config_reload(
            "rust_runner_controller_keeps_the_runner_health_across_a_config_reload",
        ),
    }
}

#[test]
fn rust_runner_controller_reopens_for_a_binary_replaced_while_the_failing_job_ran() -> TestResult<()>
{
    match child_root() {
        Some(root) => child::run(&root),
        None => runners::reopens_for_a_binary_replaced_while_the_failing_job_ran(
            "rust_runner_controller_reopens_for_a_binary_replaced_while_the_failing_job_ran",
        ),
    }
}

#[test]
fn rust_runner_controller_ends_the_official_controller_on_a_termination_signal() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => signals::run(
            "rust_runner_controller_ends_the_official_controller_on_a_termination_signal",
        ),
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
fn rust_runner_controller_holds_a_same_named_organization_set_nobody_recorded() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => organization::holds_a_matching_set_nobody_recorded(
            "rust_runner_controller_holds_a_same_named_organization_set_nobody_recorded",
        ),
    }
}

#[test]
fn rust_runner_controller_adopts_an_organization_set_after_a_recorded_intent() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => organization::adopts_a_set_after_a_recorded_intent(
            "rust_runner_controller_adopts_an_organization_set_after_a_recorded_intent",
        ),
    }
}

#[test]
fn rust_runner_controller_records_its_intent_before_creating_an_organization_set() -> TestResult<()>
{
    match child_root() {
        Some(root) => child::run(&root),
        None => organization::records_the_intent_before_it_creates_a_set(
            "rust_runner_controller_records_its_intent_before_creating_an_organization_set",
        ),
    }
}

#[test]
fn rust_runner_controller_refuses_an_organization_set_recorded_for_another_id() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => organization::refuses_a_set_the_record_assigns_to_another_id(
            "rust_runner_controller_refuses_an_organization_set_recorded_for_another_id",
        ),
    }
}

#[test]
fn rust_runner_controller_refuses_an_organization_set_with_other_labels() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => organization::refuses_a_set_with_other_labels(
            "rust_runner_controller_refuses_an_organization_set_with_other_labels",
        ),
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

#[test]
fn rust_runner_controller_holds_api_calls_until_a_primary_rate_limit_resets() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => unavailable::held_until_reset(
            "rust_runner_controller_holds_api_calls_until_a_primary_rate_limit_resets",
        ),
    }
}

#[test]
fn rust_runner_controller_admits_merge_group_offers() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => source::merge_groups("rust_runner_controller_admits_merge_group_offers"),
    }
}

#[test]
fn rust_runner_controller_retries_a_failed_jit_for_the_acquired_job() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => failure::retries_a_failed_jit_for_the_acquired_job(
            "rust_runner_controller_retries_a_failed_jit_for_the_acquired_job",
        ),
    }
}

#[test]
fn rust_runner_controller_refuses_a_jit_for_another_runner() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => failure::refuses_a_jit_for_another_runner(
            "rust_runner_controller_refuses_a_jit_for_another_runner",
        ),
    }
}

#[test]
fn rust_runner_controller_forgets_its_intent_after_a_refused_create() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => organization::forgets_its_intent_after_a_refused_create(
            "rust_runner_controller_forgets_its_intent_after_a_refused_create",
        ),
    }
}

#[test]
fn rust_runner_controller_refuses_a_created_set_that_keeps_other_labels() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => organization::refuses_a_created_set_that_keeps_other_labels(
            "rust_runner_controller_refuses_a_created_set_that_keeps_other_labels",
        ),
    }
}

#[test]
fn rust_runner_controller_waits_while_another_controller_holds_the_store() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => startup::waits_while_the_store_is_held(
            "rust_runner_controller_waits_while_another_controller_holds_the_store",
        ),
    }
}

#[test]
fn rust_runner_controller_probes_the_machine_while_idle() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => admission::probes_the_machine_while_idle(
            "rust_runner_controller_probes_the_machine_while_idle",
        ),
    }
}

#[test]
fn rust_runner_controller_closes_sessions_under_one_deadline() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => shutdown::closes_sessions_under_one_deadline(
            "rust_runner_controller_closes_sessions_under_one_deadline",
        ),
    }
}

#[test]
fn rust_runner_controller_proves_a_job_that_will_not_stop() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => shutdown::proves_a_job_that_will_not_stop(
            "rust_runner_controller_proves_a_job_that_will_not_stop",
        ),
    }
}

#[test]
fn rust_runner_controller_leaves_a_runner_of_another_scale_set() -> TestResult<()> {
    match child_root() {
        Some(root) => child::run(&root),
        None => scope::leaves_a_runner_of_another_scale_set(
            "rust_runner_controller_leaves_a_runner_of_another_scale_set",
        ),
    }
}
