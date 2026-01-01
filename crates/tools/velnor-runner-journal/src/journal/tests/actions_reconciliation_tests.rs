//! Durable Actions REST completion CAS and v7 migration tests.

use std::path::Path;

use velnor_runner_github::{
    ActionsJob, ActionsJobReconciliation, ActionsJobReconciliationState, ActionsWorkflowRun,
    InnerJob, InnerKind,
};

use crate::{HostError, Journal, Outcome};

use super::Scratch;

#[tokio::test]
async fn completed_rest_evidence_is_identity_cas_durable_and_not_cleanup_proof()
-> Result<(), String> {
    let scratch = Scratch::new("actions-rest-cas").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch_id = started_launch(&journal).await?;
    assert_eq!(
        journal
            .unbound_started_launches()
            .await
            .map_err(|error| error.to_string())?
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        vec![launch_id]
    );
    let completed = reconciliation(9001, Some("failure"));
    let pending = ActionsJobReconciliation {
        state: ActionsJobReconciliationState::Pending,
        ..completed.clone()
    };
    assert!(matches!(
        journal
            .record_actions_job_reconciliation(launch_id, &pending)
            .await,
        Err(HostError::Journal)
    ));

    journal
        .record_actions_job_reconciliation(launch_id, &completed)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_actions_job_reconciliation(launch_id, &completed)
        .await
        .map_err(|error| error.to_string())?;

    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let row = reopened
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "launch row disappeared".to_owned())?;
    assert!(row.remote_terminal);
    assert!(!row.cleanup_proven);
    assert_eq!(row.observed_workflow_run_id, Some(45));
    assert_eq!(row.observed_job_id.as_deref(), Some("opaque-scale-set-job"));
    assert_eq!(row.observed_actions_attempt, Some(2));
    assert_eq!(row.observed_actions_job_id, Some(9001));
    assert_eq!(row.observed_actions_conclusion.as_deref(), Some("failure"));

    assert!(matches!(
        reopened
            .record_actions_job_reconciliation(launch_id, &reconciliation(9002, Some("failure")))
            .await,
        Err(HostError::Journal)
    ));
    assert!(matches!(
        reopened
            .record_actions_job_reconciliation(launch_id, &reconciliation_for_runner(9001, 99))
            .await,
        Err(HostError::Journal)
    ));
    let unchanged = reopened
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "launch row disappeared after rejection".to_owned())?;
    assert_eq!(unchanged.observed_actions_job_id, Some(9001));
    assert_eq!(unchanged.observed_actions_attempt, Some(2));
    assert_eq!(
        unchanged.observed_actions_conclusion.as_deref(),
        Some("failure")
    );
    assert!(unchanged.remote_terminal);
    assert!(!unchanged.cleanup_proven);
    Ok(())
}

#[tokio::test]
async fn concurrent_conflicting_rest_completions_are_first_writer_cas() -> Result<(), String> {
    let scratch = Scratch::new("actions-rest-cas-race").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch_id = started_launch(&journal).await?;
    let left = reconciliation(9003, Some("success"));
    let right = reconciliation(9004, Some("success"));
    let (left_result, right_result) = tokio::join!(
        journal.record_actions_job_reconciliation(launch_id, &left),
        journal.record_actions_job_reconciliation(launch_id, &right),
    );
    assert_ne!(left_result.is_ok(), right_result.is_ok());

    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let row = reopened
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "launch row disappeared after concurrent CAS".to_owned())?;
    assert!(matches!(row.observed_actions_job_id, Some(9003 | 9004)));
    assert_eq!(row.observed_actions_attempt, Some(2));
    assert!(row.remote_terminal);
    assert!(!row.cleanup_proven);
    Ok(())
}

#[tokio::test]
async fn v7_upgrade_preserves_uncertain_launch_and_adds_empty_rest_evidence() -> Result<(), String>
{
    let scratch = Scratch::new("journal-v7-to-v10").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch_id = journal
        .begin("launch", "uncertain-v7")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_launch_effect_intent(launch_id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(launch_id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);

    seed_v7_schema_without_rest_columns(&path).await?;
    let migrated = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    let mut version_rows = conn
        .query("PRAGMA user_version", ())
        .await
        .map_err(|error| error.to_string())?;
    let version_row = version_rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "journal version row missing".to_owned())?;
    assert_eq!(
        version_row
            .get::<i64>(0)
            .map_err(|error| error.to_string())?,
        13
    );
    drop(version_rows);
    drop(conn);
    drop(database);
    let row = migrated
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "uncertain launch disappeared in migration".to_owned())?;
    assert_eq!(row.state, crate::IntentState::Uncertain);
    assert_eq!(
        row.launch_effect,
        crate::journal::LaunchEffectState::MayHaveEffect
    );
    assert!(!row.cleanup_proven);
    assert!(!row.remote_terminal);
    assert_eq!(row.observed_actions_attempt, None);
    assert_eq!(row.observed_actions_job_id, None);
    assert_eq!(row.observed_actions_conclusion, None);
    Ok(())
}

pub(super) async fn started_launch(journal: &Journal) -> Result<i64, String> {
    let launch_id = prepared_launch(journal).await?;
    if !journal
        .observe_runner_event(&lifecycle_event(InnerKind::Started))
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("started event did not match the test launch".to_owned());
    }
    Ok(launch_id)
}

pub(super) async fn prepared_launch(journal: &Journal) -> Result<i64, String> {
    let (launch_id, created) = journal
        .begin_launch("actions-rest-lifecycle")
        .await
        .map_err(|error| error.to_string())?;
    if !created {
        return Err("unexpected replay of new launch test fixture".to_owned());
    }
    journal
        .bind_launch_identity(launch_id, None, None, None, None, "runner-v8")
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
        .record_outer_network_intent(launch_id, "worker-outer-test")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_outer_network_id(launch_id, &"c".repeat(64))
        .await
        .map_err(|error| error.to_string())?;
    let runner_container = "a".repeat(64);
    let dind_container = "b".repeat(64);
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
        .finish(launch_id, Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    Ok(launch_id)
}

pub(super) fn lifecycle_event(kind: InnerKind) -> InnerJob {
    InnerJob {
        kind,
        request_id: None,
        job_id: Some("opaque-scale-set-job".to_owned()),
        workflow_run_id: Some(45),
        owner_name: Some("acme".to_owned()),
        repository_name: Some("runner".to_owned()),
        event_name: Some("push".to_owned()),
        labels: Vec::new(),
        runner_id: Some(88),
        runner_name: Some("runner-v8".to_owned()),
        result: None,
        fields: Vec::new(),
    }
}

pub(super) fn reconciliation(
    actions_job_id: i64,
    conclusion: Option<&str>,
) -> ActionsJobReconciliation {
    reconciliation_for_runner_with_conclusion(actions_job_id, 88, conclusion)
}

fn reconciliation_for_runner(actions_job_id: i64, runner_id: i64) -> ActionsJobReconciliation {
    reconciliation_for_runner_with_conclusion(actions_job_id, runner_id, Some("failure"))
}

fn reconciliation_for_runner_with_conclusion(
    actions_job_id: i64,
    runner_id: i64,
    conclusion: Option<&str>,
) -> ActionsJobReconciliation {
    ActionsJobReconciliation {
        state: ActionsJobReconciliationState::Completed,
        scale_set_job_id: Some("opaque-scale-set-job".to_owned()),
        observed_workflow_run_id: Some(45),
        observed_runner_id: Some(runner_id),
        observed_runner_name: Some("runner-v8".to_owned()),
        attempt: Some(2),
        job: Some(ActionsJob {
            id: actions_job_id,
            run_id: 45,
            status: "completed".to_owned(),
            conclusion: conclusion.map(str::to_owned),
            runner_id: Some(runner_id),
            runner_name: Some("runner-v8".to_owned()),
            runner_group_id: Some(5),
            runner_group_name: Some("protected".to_owned()),
        }),
        workflow_run: Some(ActionsWorkflowRun {
            id: 45,
            path: ".github/workflows/test.yml@refs/heads/main".to_owned(),
            run_attempt: 2,
            status: "completed".to_owned(),
            conclusion: conclusion.map(str::to_owned),
            event: "push".to_owned(),
            head_sha: "0123456789abcdef".to_owned(),
            head_repository_full_name: Some("acme/runner".to_owned()),
        }),
        reason: None,
    }
}

async fn seed_v7_schema_without_rest_columns(path: &Path) -> Result<(), String> {
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE linux_launch_daemon_bindings", ())
        .await
        .map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE linux_launch_daemon_adoptions", ())
        .await
        .map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE linux_launch_started_observations", ())
        .await
        .map_err(|error| error.to_string())?;
    for column in [
        "observed_actions_conclusion",
        "observed_actions_job_id",
        "observed_actions_attempt",
    ] {
        conn.execute(&format!("ALTER TABLE intents DROP COLUMN {column}"), ())
            .await
            .map_err(|error| error.to_string())?;
    }
    conn.execute("PRAGMA user_version = 7", ())
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}
