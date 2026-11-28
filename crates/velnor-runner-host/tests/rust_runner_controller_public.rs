// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "rust_runner_controller_public/acquire_shape.rs"]
mod acquire_shape;
#[path = "rust_runner_controller_public/admission.rs"]
mod admission;
#[path = "rust_runner_controller_public/assigned.rs"]
mod assigned;
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
#[path = "rust_runner_controller_public/session_401.rs"]
mod session_401;
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

macro_rules! public_test {
    ($name:ident, $run:path) => {
        #[test]
        fn $name() -> TestResult<()> {
            match child_root() {
                Some(root) => child::run(&root),
                None => $run(stringify!($name)),
            }
        }
    };
}

public_test!(rust_runner_controller_runs_authorized_job_once, once::run);
public_test!(
    rust_runner_controller_goes_to_jit_for_an_assigned_offer,
    assigned::goes_to_jit
);
public_test!(
    rust_runner_controller_goes_to_jit_for_an_assigned_offer_that_supersedes_an_available_one,
    assigned::goes_to_jit_after_an_available_one
);
public_test!(
    rust_runner_controller_never_acquires_unapproved_source,
    source::run
);
public_test!(
    rust_runner_controller_holds_slot_until_cleanup_proven,
    slot::run
);
public_test!(
    rust_runner_controller_sweeps_machine_before_admission,
    sweep::run
);
public_test!(rust_runner_controller_stops_jobs_on_shutdown, shutdown::run);
public_test!(
    rust_runner_controller_proves_cleanup_after_a_failed_launch,
    failure::run
);
public_test!(
    rust_runner_controller_defers_a_check_github_could_not_answer,
    unavailable::run
);
public_test!(
    rust_runner_controller_defers_a_rate_limited_check,
    unavailable::rate_limited
);
public_test!(
    rust_runner_controller_refuses_a_403_without_rate_limit_signals,
    unavailable::refused_for_good
);
public_test!(
    rust_runner_controller_degrades_after_three_failed_runner_exits,
    runners::run
);
public_test!(
    rust_runner_controller_keeps_the_runner_health_across_a_config_reload,
    runners::keeps_the_health_across_a_config_reload
);
public_test!(
    rust_runner_controller_reopens_for_a_binary_replaced_while_the_failing_job_ran,
    runners::reopens_for_a_binary_replaced_while_the_failing_job_ran
);
public_test!(
    rust_runner_controller_ends_the_official_controller_on_a_termination_signal,
    signals::run
);
public_test!(
    rust_runner_controller_closes_admission_on_drain_and_config_change,
    admission::run
);

#[test]
fn rust_runner_controller_reports_stale_heartbeats_as_degraded() -> TestResult<()> {
    heartbeat::run()
}

public_test!(
    rust_runner_controller_holds_a_same_named_organization_set_nobody_recorded,
    organization::holds_a_matching_set_nobody_recorded
);
public_test!(
    rust_runner_controller_adopts_an_organization_set_after_a_recorded_intent,
    organization::adopts_a_set_after_a_recorded_intent
);
public_test!(
    rust_runner_controller_records_its_intent_before_creating_an_organization_set,
    organization::records_the_intent_before_it_creates_a_set
);
public_test!(
    rust_runner_controller_refuses_an_organization_set_recorded_for_another_id,
    organization::refuses_a_set_the_record_assigns_to_another_id
);
public_test!(
    rust_runner_controller_refuses_an_organization_set_with_other_labels,
    organization::refuses_a_set_with_other_labels
);
public_test!(
    rust_runner_controller_leaves_a_held_startup_when_host_toml_changes,
    startup::run
);
public_test!(
    rust_runner_controller_reports_a_failing_session_refresh_as_degraded,
    faults::degraded_session
);
public_test!(
    rust_runner_controller_cleans_up_after_a_controller_error,
    faults::cleans_up_after_an_error
);
public_test!(
    rust_runner_controller_finishes_a_stop_after_a_handler_fails,
    faults::finishes_a_stop_after_a_handler_fails
);
public_test!(
    rust_runner_controller_does_no_cleanup_after_losing_the_daemon_lock,
    faults::leaves_everything_after_a_lost_lock
);
public_test!(
    rust_runner_controller_reads_host_toml_up_to_the_shared_limit,
    limit::run
);
public_test!(
    rust_runner_controller_ends_an_orb_call_that_hangs,
    deadline::run
);
public_test!(
    rust_runner_controller_holds_api_calls_until_a_primary_rate_limit_resets,
    unavailable::held_until_reset
);
public_test!(
    rust_runner_controller_admits_merge_group_offers,
    source::merge_groups
);
public_test!(
    rust_runner_controller_retries_a_failed_jit_for_the_acquired_job,
    failure::retries_a_failed_jit_for_the_acquired_job
);
public_test!(
    rust_runner_controller_refuses_a_jit_for_another_runner,
    failure::refuses_a_jit_for_another_runner
);
public_test!(
    rust_runner_controller_forgets_its_intent_after_a_refused_create,
    organization::forgets_its_intent_after_a_refused_create
);
public_test!(
    rust_runner_controller_refuses_a_created_set_that_keeps_other_labels,
    organization::refuses_a_created_set_that_keeps_other_labels
);
public_test!(
    rust_runner_controller_waits_while_another_controller_holds_the_store,
    startup::waits_while_the_store_is_held
);
public_test!(
    rust_runner_controller_probes_the_machine_while_idle,
    admission::probes_the_machine_while_idle
);
public_test!(
    rust_runner_controller_closes_sessions_under_one_deadline,
    shutdown::closes_sessions_under_one_deadline
);
public_test!(
    rust_runner_controller_proves_a_job_that_will_not_stop,
    shutdown::proves_a_job_that_will_not_stop
);
public_test!(
    rust_runner_controller_leaves_a_runner_of_another_scale_set,
    scope::leaves_a_runner_of_another_scale_set
);
