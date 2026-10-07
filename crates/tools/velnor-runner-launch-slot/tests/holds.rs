//! Slot-hold truth table: launch rows hold until cleanup or failure.

use velnor_runner_host::IntentState;
use velnor_runner_journal::journal::LaunchEffectState;
use velnor_runner_journal::reconcile::IntentRow;
use velnor_runner_launch_slot::holds;

fn row(kind: &str, state: IntentState, cleanup_proven: bool) -> IntentRow {
    IntentRow {
        id: 1,
        kind: kind.to_owned(),
        subject: "subject".to_owned(),
        state,
        launch_effect: LaunchEffectState::Unknown,
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
        remote_terminal: false,
        cleanup_proven,
    }
}

#[test]
fn launch_row_holds_until_cleanup_proven() {
    for state in [
        IntentState::Pending,
        IntentState::Done,
        IntentState::Uncertain,
    ] {
        assert!(holds(&row("launch", state, false)));
    }
}

#[test]
fn failed_rows_never_hold() {
    let mut no_effect = row("launch", IntentState::Failed, false);
    no_effect.launch_effect = LaunchEffectState::DefiniteNoEffect;
    assert!(!holds(&no_effect));
}

#[test]
fn failed_rows_with_unknown_or_dispatched_effects_still_hold() {
    for effect in [LaunchEffectState::Unknown, LaunchEffectState::MayHaveEffect] {
        let mut failed = row("launch", IntentState::Failed, false);
        failed.launch_effect = effect;
        assert!(holds(&failed), "{effect:?}");
    }
}

#[test]
fn proven_cleanup_releases_hold() {
    assert!(!holds(&row("launch", IntentState::Done, true)));
}

#[test]
fn foreign_kinds_never_hold() {
    for kind in ["acquire", "delete", "Launch", ""] {
        assert!(!holds(&row(kind, IntentState::Done, false)), "{kind}");
    }
}
