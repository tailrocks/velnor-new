use std::time::Duration;

use velnor_runner_github::{
    ActionsJob, ActionsJobReconciliation, ActionsJobReconciliationState, ActionsWorkflowRun,
    InnerJob, InnerKind,
};
use velnor_runner_host::IntentRow;
use velnor_runner_host::worker::OwnedDockerResource;
use velnor_runner_journal::journal::{Journal, JournalDockerDaemonBinding};
use velnor_runner_launch_slot::holds;

use crate::launch::harness::Scratch;
use crate::linux::session::ShutdownGate;

use super::super::{RecoveryBudget, run_recovery_pass};
use super::{OUTER_NETWORK, RUNNER_ID, RUNNER_NAME, SCALE_SET_JOB_ID, ScriptedRecovery};

pub(super) async fn launch_fixture(
    name: &str,
) -> Result<
    (
        Scratch,
        std::path::PathBuf,
        Journal,
        i64,
        JournalDockerDaemonBinding,
    ),
    String,
> {
    let scratch = Scratch::new(name).map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch_id = prepare_started_launch(&journal).await?;
    let binding = JournalDockerDaemonBinding::new("/run/docker.sock", "logical-engine-a")
        .map_err(|error| error.to_string())?;
    Ok((scratch, path, journal, launch_id, binding))
}

pub(super) fn scripted<R, I>(
    journal: &Journal,
    binding: JournalDockerDaemonBinding,
    rest: R,
    inventories: I,
    inventory_must_see_receipt: bool,
) -> ScriptedRecovery<'_>
where
    R: IntoIterator<Item = Option<ActionsJobReconciliation>>,
    I: IntoIterator<Item = Option<Vec<OwnedDockerResource>>>,
{
    ScriptedRecovery {
        journal,
        rest: rest.into_iter().collect(),
        inventories: inventories.into_iter().collect(),
        observed_binding: binding,
        inventory_must_see_receipt,
        rest_calls: 0,
        inventory_calls: 0,
        rest_pends: false,
        rest_dropped: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    }
}

pub(super) async fn run_scripted_pass(
    journal: &Journal,
    binding: &JournalDockerDaemonBinding,
    services: &mut ScriptedRecovery<'_>,
    budget: &mut RecoveryBudget,
) -> Option<Vec<i64>> {
    let (_sender, mut receiver) = tokio::sync::watch::channel(None);
    let mut cutoff = None;
    let mut shutdown = ShutdownGate {
        receiver: &mut receiver,
        cutoff: &mut cutoff,
    };
    run_recovery_pass(
        journal,
        binding,
        services,
        budget,
        Duration::from_secs(5),
        &mut shutdown,
    )
    .await
}

pub(super) async fn assert_still_held_unbound(
    journal: &Journal,
    launch_id: i64,
) -> Result<(), String> {
    let row = launch_row(journal, launch_id).await?;
    assert!(row.remote_terminal);
    assert_eq!(row.observed_actions_attempt, Some(2));
    assert_eq!(row.observed_actions_job_id, Some(9001));
    assert!(!row.cleanup_proven);
    assert!(holds(&row));
    assert!(
        journal
            .launch_daemon_binding(launch_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        1
    );
    Ok(())
}

pub(super) async fn assert_still_unstarted_and_held(
    journal: &Journal,
    launch_id: i64,
) -> Result<(), String> {
    let row = launch_row(journal, launch_id).await?;
    assert!(!row.remote_terminal);
    assert_eq!(row.observed_actions_job_id, None);
    assert!(!row.cleanup_proven);
    assert!(holds(&row));
    assert!(
        journal
            .launch_daemon_binding(launch_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

pub(super) async fn prepare_started_launch(journal: &Journal) -> Result<i64, String> {
    let runner_container = "a".repeat(64);
    let dind_container = "b".repeat(64);
    let outer_network_id = "c".repeat(64);
    let (launch_id, created) = journal
        .begin_launch("option-a-recovery")
        .await
        .map_err(|error| error.to_string())?;
    if !created {
        return Err("expected a new legacy launch generation".to_owned());
    }
    journal
        .bind_launch_identity(launch_id, None, None, None, None, RUNNER_NAME)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_launch_effect_intent(launch_id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker_volume(launch_id, "worker-volume-test")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_outer_network_intent(launch_id, OUTER_NETWORK)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_outer_network_id(launch_id, &outer_network_id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind(launch_id, Some(&runner_container), Some("88"))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_worker(launch_id, Some(&runner_container), Some(&dind_container))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_start_intent(launch_id, &runner_container)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(launch_id, velnor_runner_journal::Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    let event = InnerJob {
        kind: InnerKind::Started,
        request_id: None,
        job_id: Some(SCALE_SET_JOB_ID.to_owned()),
        workflow_run_id: Some(45),
        owner_name: Some("acme".to_owned()),
        repository_name: Some("runner".to_owned()),
        event_name: Some("push".to_owned()),
        labels: Vec::new(),
        runner_id: Some(RUNNER_ID),
        runner_name: Some(RUNNER_NAME.to_owned()),
        result: None,
        fields: Vec::new(),
    };
    if !journal
        .observe_runner_event(&event)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("actual Started identity did not match its launch generation".to_owned());
    }
    Ok(launch_id)
}

pub(super) async fn launch_row(journal: &Journal, launch_id: i64) -> Result<IntentRow, String> {
    journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "launch generation disappeared".to_owned())
}

pub(super) fn completed_reconciliation() -> ActionsJobReconciliation {
    ActionsJobReconciliation {
        state: ActionsJobReconciliationState::Completed,
        scale_set_job_id: Some(SCALE_SET_JOB_ID.to_owned()),
        observed_workflow_run_id: Some(45),
        observed_runner_id: Some(RUNNER_ID),
        observed_runner_name: Some(RUNNER_NAME.to_owned()),
        attempt: Some(2),
        job: Some(ActionsJob {
            id: 9001,
            run_id: 45,
            status: "completed".to_owned(),
            conclusion: Some("failure".to_owned()),
            runner_id: Some(RUNNER_ID),
            runner_name: Some(RUNNER_NAME.to_owned()),
            runner_group_id: Some(5),
            runner_group_name: Some("protected".to_owned()),
        }),
        workflow_run: Some(ActionsWorkflowRun {
            id: 45,
            path: ".github/workflows/test.yml@refs/heads/main".to_owned(),
            run_attempt: 2,
            status: "completed".to_owned(),
            conclusion: Some("failure".to_owned()),
            event: "push".to_owned(),
            head_sha: "0123456789abcdef".to_owned(),
            head_repository_full_name: Some("acme/runner".to_owned()),
        }),
        reason: None,
    }
}
