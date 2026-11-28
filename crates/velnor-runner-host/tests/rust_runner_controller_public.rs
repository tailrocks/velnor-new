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
#[path = "rust_runner_controller_public/claimant.rs"]
mod claimant;
#[path = "rust_runner_controller_public/connection_drain.rs"]
mod connection_drain;
#[path = "rust_runner_controller_public/credentials.rs"]
mod credentials;
#[path = "rust_runner_controller_public/deadline.rs"]
mod deadline;
#[path = "rust_runner_controller_public/detach_scope.rs"]
mod detach_scope;
#[path = "rust_runner_controller_public/disconnect.rs"]
mod disconnect;
#[path = "rust_runner_controller_public/disconnect_set.rs"]
mod disconnect_set;
#[path = "rust_runner_controller_public/doctor_probe.rs"]
mod doctor_probe;
#[path = "rust_runner_controller_public/drain_wait.rs"]
mod drain_wait;
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
#[path = "rust_runner_controller_public/last_connection.rs"]
mod last_connection;
#[path = "rust_runner_controller_public/limit.rs"]
mod limit;
#[path = "rust_runner_controller_public/lost_create.rs"]
mod lost_create;
#[path = "rust_runner_controller_public/once.rs"]
mod once;
#[path = "rust_runner_controller_public/organization.rs"]
mod organization;
#[path = "rust_runner_controller_public/retire.rs"]
mod retire;
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
#[path = "rust_runner_controller_public/world_files.rs"]
mod world_files;

use fixtures::TestResult;

fn child_root() -> Option<std::path::PathBuf> {
    std::env::var_os(harness::CHILD_MARKER).map(Into::into)
}

macro_rules! controller_test {
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

controller_test!(rust_runner_controller_runs_authorized_job_once, once::run);

controller_test!(
    rust_runner_controller_never_acquires_unapproved_source,
    source::run
);

controller_test!(
    rust_runner_controller_holds_slot_until_cleanup_proven,
    slot::run
);

controller_test!(
    rust_runner_controller_sweeps_machine_before_admission,
    sweep::run
);

controller_test!(rust_runner_controller_stops_jobs_on_shutdown, shutdown::run);

controller_test!(
    rust_runner_controller_proves_cleanup_after_a_failed_launch,
    failure::run
);

controller_test!(
    rust_runner_controller_defers_a_check_github_could_not_answer,
    unavailable::run
);

controller_test!(
    rust_runner_controller_defers_a_rate_limited_check,
    unavailable::rate_limited
);

controller_test!(
    rust_runner_controller_refuses_a_403_without_rate_limit_signals,
    unavailable::refused_for_good
);

controller_test!(
    rust_runner_controller_degrades_after_three_failed_runner_exits,
    runners::run
);

controller_test!(
    rust_runner_controller_keeps_the_runner_health_across_a_config_reload,
    runners::keeps_the_health_across_a_config_reload
);

controller_test!(
    rust_runner_controller_reopens_for_a_binary_replaced_while_the_failing_job_ran,
    runners::reopens_for_a_binary_replaced_while_the_failing_job_ran
);

controller_test!(
    rust_runner_controller_ends_the_official_controller_on_a_termination_signal,
    signals::run
);

controller_test!(
    rust_runner_controller_closes_admission_on_drain_and_config_change,
    admission::run
);

#[test]
fn rust_runner_controller_reports_stale_heartbeats_as_degraded() -> TestResult<()> {
    heartbeat::run()
}

controller_test!(
    rust_runner_controller_holds_a_same_named_organization_set_nobody_recorded,
    organization::holds_a_matching_set_nobody_recorded
);

controller_test!(
    rust_runner_controller_adopts_an_organization_set_after_a_recorded_intent,
    organization::adopts_a_set_after_a_recorded_intent
);

controller_test!(
    rust_runner_controller_records_its_intent_before_creating_an_organization_set,
    organization::records_the_intent_before_it_creates_a_set
);

controller_test!(
    rust_runner_controller_refuses_an_organization_set_recorded_for_another_id,
    organization::refuses_a_set_the_record_assigns_to_another_id
);

controller_test!(
    rust_runner_controller_holds_a_recorded_set_the_listing_does_not_show,
    organization::holds_a_recorded_set_the_listing_does_not_show
);

controller_test!(
    rust_runner_controller_holds_a_repository_set_listed_under_another_id_than_its_record,
    organization::holds_a_repository_set_listed_under_another_id_than_its_record
);

controller_test!(
    rust_runner_controller_refuses_an_organization_set_with_other_labels,
    organization::refuses_a_set_with_other_labels
);

controller_test!(
    rust_runner_controller_leaves_a_held_startup_when_host_toml_changes,
    startup::run
);

controller_test!(
    rust_runner_controller_reports_a_failing_session_refresh_as_degraded,
    faults::degraded_session
);

controller_test!(
    rust_runner_controller_cleans_up_after_a_controller_error,
    faults::cleans_up_after_an_error
);

controller_test!(
    rust_runner_controller_does_no_cleanup_after_losing_the_daemon_lock,
    faults::leaves_everything_after_a_lost_lock
);

controller_test!(
    rust_runner_controller_reads_host_toml_up_to_the_shared_limit,
    limit::run
);

controller_test!(
    rust_runner_controller_ends_an_orb_call_that_hangs,
    deadline::run
);

controller_test!(
    rust_runner_controller_holds_api_calls_until_a_primary_rate_limit_resets,
    unavailable::held_until_reset
);

controller_test!(
    rust_runner_controller_admits_merge_group_offers,
    source::merge_groups
);

controller_test!(
    rust_runner_controller_retries_a_failed_jit_for_the_acquired_job,
    failure::retries_a_failed_jit_for_the_acquired_job
);

controller_test!(
    rust_runner_controller_refuses_a_jit_for_another_runner,
    failure::refuses_a_jit_for_another_runner
);

controller_test!(
    rust_runner_controller_forgets_its_intent_after_a_refused_create,
    organization::forgets_its_intent_after_a_refused_create
);

controller_test!(
    rust_runner_controller_refuses_a_created_set_that_keeps_other_labels,
    organization::refuses_a_created_set_that_keeps_other_labels
);

controller_test!(
    rust_runner_controller_waits_while_another_controller_holds_the_store,
    startup::waits_while_the_store_is_held
);

controller_test!(
    rust_runner_controller_probes_the_machine_while_idle,
    admission::probes_the_machine_while_idle
);

controller_test!(
    rust_runner_controller_closes_sessions_under_one_deadline,
    shutdown::closes_sessions_under_one_deadline
);

controller_test!(
    rust_runner_controller_proves_a_job_that_will_not_stop,
    shutdown::proves_a_job_that_will_not_stop
);

controller_test!(
    rust_runner_controller_leaves_a_runner_of_another_scale_set,
    scope::leaves_a_runner_of_another_scale_set
);

controller_test!(
    rust_runner_drain_wait_proves_every_slot_cleaned_by_a_fresh_census,
    drain_wait::run
);

controller_test!(
    rust_runner_disconnect_drains_one_connection_and_removes_its_set,
    disconnect::run
);

controller_test!(
    rust_runner_disconnect_deletes_only_an_unclaimed_set_it_created,
    disconnect_set::run
);

controller_test!(
    rust_runner_disconnect_leaves_a_set_it_did_not_create,
    last_connection::run
);

controller_test!(
    rust_runner_doctor_probe_names_each_failed_check,
    doctor_probe::run
);

controller_test!(
    rust_runner_connection_drain_and_reconnect_follow_the_census_of_the_controller,
    connection_drain::run
);

controller_test!(
    rust_runner_detach_stops_at_its_budget_and_ignores_an_organization_set_of_the_same_name,
    detach_scope::run
);

controller_test!(
    rust_runner_removed_connection_closes_its_session_once_idle_and_retries_a_failed_close,
    retire::removal
);

controller_test!(
    rust_runner_restarts_when_a_removal_comes_with_another_change,
    retire::restart
);

controller_test!(
    rust_runner_serves_a_connection_again_when_its_removal_is_reverted_before_the_close,
    retire::revert
);

controller_test!(
    rust_runner_detach_hands_the_set_to_the_connection_that_replaced_it,
    claimant::run
);

controller_test!(
    rust_runner_detach_holds_the_set_while_the_replacing_connection_has_another_record,
    claimant::conflict
);

controller_test!(
    rust_runner_detach_waits_for_a_reconciled_census,
    claimant::unreconciled
);

controller_test!(
    rust_runner_startup_holds_a_changed_config_until_every_session_is_closed,
    startup::holds_a_changed_config_until_the_session_is_closed
);

controller_test!(
    rust_runner_disconnect_removes_a_set_whose_create_response_was_lost,
    lost_create::run
);

controller_test!(
    rust_runner_startup_counts_a_404_after_a_timed_out_close_as_closed,
    startup::counts_a_404_after_a_timed_out_close_as_closed
);
