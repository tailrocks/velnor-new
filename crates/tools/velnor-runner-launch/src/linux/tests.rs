use std::time::{Duration, Instant};

use velnor_runner_host::worker::{OwnedDockerResource, OwnedDockerResourceKind};
use velnor_runner_host::{IntentRow, IntentState};
use velnor_runner_journal::journal::{
    IntentState as JournalIntentState, Journal, LaunchEffectState, Outcome, RunnerStartIntent,
};

use super::shutdown::{
    RowCounts, cleanup_identity, outcome_with_counts, retained_cutoff, row_counts,
    summarize_complete_snapshot, summarize_snapshot_before_deadline,
};
use super::{LinuxAdmissionState, LinuxDaemonOutcome, LinuxLaunchCredentials, LinuxShutdownGap};

#[path = "tests/protected_state.rs"]
mod protected_state;

#[test]
fn shutdown_counts_keep_uncertain_launches_sessions_and_auth_steps() {
    let rows = vec![
        row(
            "launch",
            IntentState::Pending,
            LaunchEffectState::NotStarted,
            false,
        ),
        row(
            "launch",
            IntentState::Failed,
            LaunchEffectState::DefiniteNoEffect,
            false,
        ),
        row(
            "launch",
            IntentState::Failed,
            LaunchEffectState::MayHaveEffect,
            false,
        ),
        row(
            "discovery-credential",
            IntentState::Pending,
            LaunchEffectState::MayHaveEffect,
            false,
        ),
        row(
            "discovery-credential",
            IntentState::Done,
            LaunchEffectState::MayHaveEffect,
            false,
        ),
        row(
            "session",
            IntentState::Done,
            LaunchEffectState::MayHaveEffect,
            false,
        ),
        row(
            "session",
            IntentState::Done,
            LaunchEffectState::MayHaveEffect,
            true,
        ),
        row(
            "unknown-kind",
            IntentState::Failed,
            LaunchEffectState::DefiniteNoEffect,
            false,
        ),
    ];

    assert_eq!(
        row_counts(&rows),
        RowCounts {
            occupied_launches: 2,
            unresolved_intents: 2,
        }
    );
}

#[test]
fn complete_empty_inventory_is_required_for_quiescent() {
    let admission = LinuxAdmissionState::RunnerProfileUnavailable;
    assert_eq!(
        summarize_complete_snapshot(admission, &[], &[], 0),
        LinuxDaemonOutcome::Quiescent {
            admission,
            cleaned_generations: 0,
        }
    );

    let resources = [OwnedDockerResource {
        kind: OwnedDockerResourceKind::Network,
        id_or_name: "network-1".to_owned(),
        names: vec!["worker-1-outer".to_owned()],
        worker: "worker-1".to_owned(),
        role: "outer-network".to_owned(),
        labels: std::collections::BTreeMap::default(),
    }];
    assert_eq!(
        summarize_complete_snapshot(admission, &[], &resources, 0),
        LinuxDaemonOutcome::Unresolved {
            admission,
            gap: LinuxShutdownGap::OwnedResourcesRemain,
            occupied_launches: Some(0),
            unresolved_intents: Some(0),
            owned_resources: Some(1),
            cleaned_generations: 0,
        }
    );

    let unresolved = [row(
        "discovery-credential",
        IntentState::Uncertain,
        LaunchEffectState::MayHaveEffect,
        false,
    )];
    assert_eq!(
        summarize_complete_snapshot(admission, &unresolved, &[], 0),
        LinuxDaemonOutcome::Unresolved {
            admission,
            gap: LinuxShutdownGap::UnresolvedIntent,
            occupied_launches: Some(0),
            unresolved_intents: Some(1),
            owned_resources: Some(0),
            cleaned_generations: 0,
        }
    );
}

#[test]
fn deadline_outcome_keeps_unknown_counts_instead_of_zero_defaults() {
    let expired = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("monotonic instant supports a one-second prior point");
    assert_eq!(
        outcome_with_counts(
            LinuxAdmissionState::PoolPreflightUnavailable,
            LinuxShutdownGap::JournalUnavailable,
            None,
            None,
            0,
            Some(expired),
        ),
        LinuxDaemonOutcome::Deadline {
            admission: LinuxAdmissionState::PoolPreflightUnavailable,
            occupied_launches: None,
            unresolved_intents: None,
            gap: LinuxShutdownGap::JournalUnavailable,
            owned_resources: None,
        }
    );
}

#[test]
fn cleanup_requires_actual_terminal_event_and_complete_owned_generation() {
    let mut complete = row(
        "launch",
        IntentState::Done,
        LaunchEffectState::MayHaveEffect,
        false,
    );
    complete.remote_terminal = true;
    complete.runner_start_intent = RunnerStartIntent::MayHaveStarted;
    complete.runner_name = Some("runner-1".to_owned());
    complete.worker_volume = Some("worker-1".to_owned());
    complete.docker_id = Some("a".repeat(64));
    complete.dind_id = Some("b".repeat(64));
    complete.github_runner_id = Some("42".to_owned());
    complete.observed_workflow_run_id = Some(7);
    complete.observed_job_id = Some("opaque-scale-set-job".to_owned());
    assert!(cleanup_identity(&complete).is_some());

    let mut no_terminal = complete.clone();
    no_terminal.remote_terminal = false;
    assert!(cleanup_identity(&no_terminal).is_none());

    let mut incomplete = complete;
    incomplete.dind_id = None;
    assert!(cleanup_identity(&incomplete).is_none());
}

#[test]
fn credentials_are_redacted_and_validate_each_role() {
    let credentials =
        LinuxLaunchCredentials::new("controller-secret".to_owned(), "actions-secret".to_owned())
            .expect("valid credentials");
    let debug = format!("{credentials:?}");
    assert!(!debug.contains("controller-secret"));
    assert!(!debug.contains("actions-secret"));
    assert!(LinuxLaunchCredentials::new(String::new(), "actions-secret".to_owned()).is_err());
    assert!(
        LinuxLaunchCredentials::new("controller-secret".to_owned(), "bad\ntoken".to_owned())
            .is_err()
    );
}

#[test]
fn shutdown_channel_close_retains_the_first_signal_cutoff() {
    let first = Instant::now()
        .checked_add(Duration::from_secs(9))
        .expect("monotonic instant supports a short future cutoff");
    assert_eq!(
        retained_cutoff(Some(first), true, Duration::from_secs(60), Instant::now()),
        Some(first)
    );

    let started = Instant::now();
    assert_eq!(
        retained_cutoff(None, true, Duration::from_secs(3), started),
        started.checked_add(Duration::from_secs(3))
    );
    assert_eq!(
        retained_cutoff(None, false, Duration::from_secs(3), started),
        None
    );
}

#[tokio::test]
async fn shutdown_reads_persisted_intents_and_confirms_the_durable_fence() -> Result<(), String> {
    let scratch = crate::launch::harness::Scratch::new("linux-shutdown-journal")
        .map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let session_id = journal
        .begin("session", "uncertain-shutdown-session")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(session_id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;

    assert!(!super::shutdown::confirm_drain(&journal, Instant::now()).await);
    assert_eq!(journal.draining().await, Ok(false));
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .ok_or_else(|| "monotonic deadline overflowed".to_owned())?;
    assert!(super::shutdown::confirm_drain(&journal, deadline).await);
    assert_eq!(journal.draining().await, Ok(true));
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, JournalIntentState::Uncertain);
    assert_eq!(row_counts(&rows).unresolved_intents, 1);
    drop(journal);

    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(reopened.draining().await, Ok(true));
    let reopened_rows = reopened.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(reopened_rows[0].state, JournalIntentState::Uncertain);
    assert_eq!(row_counts(&reopened_rows).unresolved_intents, 1);
    Ok(())
}

fn row(
    kind: &str,
    state: IntentState,
    launch_effect: LaunchEffectState,
    cleanup_proven: bool,
) -> IntentRow {
    IntentRow {
        id: 1,
        kind: kind.to_owned(),
        subject: "fixture".to_owned(),
        state,
        launch_effect,
        docker_id: None,
        dind_id: None,
        worker_volume: None,
        github_runner_id: None,
        message_id: None,
        runner_request_id: None,
        requested_workflow_run_id: None,
        requested_job_id: None,
        runner_name: None,
        observed_job_id: None,
        observed_workflow_run_id: None,
        observed_actions_attempt: None,
        observed_actions_job_id: None,
        observed_actions_conclusion: None,
        remote_terminal: false,
        cleanup_proven,
        outer_network_name: None,
        outer_network_id: None,
        runner_start_intent: RunnerStartIntent::UnknownLegacy,
    }
}

#[test]
fn final_snapshot_after_cutoff_is_never_quiescent() {
    let admission = LinuxAdmissionState::RunnerProfileUnavailable;
    let rows = [];
    let resources = [];
    let expired = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("monotonic instant supports a one-second prior point");
    assert_eq!(
        summarize_snapshot_before_deadline(admission, &rows, &resources, 0, expired),
        LinuxDaemonOutcome::Deadline {
            admission,
            occupied_launches: Some(0),
            unresolved_intents: Some(0),
            gap: LinuxShutdownGap::QuiescenceSnapshotPastDeadline,
            owned_resources: Some(0),
        }
    );
}
