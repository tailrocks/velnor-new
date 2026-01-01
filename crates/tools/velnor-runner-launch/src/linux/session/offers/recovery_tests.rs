use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use velnor_runner_github::ActionsJobReconciliation;
use velnor_runner_host::IntentRow;
use velnor_runner_host::worker::OwnedDockerResource;
use velnor_runner_journal::journal::{Journal, JournalDockerDaemonBinding};
use velnor_runner_launch_slot::holds;

#[path = "recovery_inventory_fixtures.rs"]
mod inventory_fixtures;

use super::{RecoveryBudget, RecoveryFuture, RecoveryServices, run_recovery_pass};
use crate::linux::session::ShutdownGate;

#[path = "recovery_test_support.rs"]
mod support;

use support::{
    assert_still_held_unbound, assert_still_unstarted_and_held, completed_reconciliation,
    launch_fixture, launch_row, run_scripted_pass, scripted,
};

const RUNNER_ID: i64 = 88;
const RUNNER_NAME: &str = "runner-v8";
const SCALE_SET_JOB_ID: &str = "opaque-scale-set-job";
const OUTER_NETWORK: &str = "worker-outer-test";

struct ScriptedRecovery<'a> {
    journal: &'a Journal,
    rest: VecDeque<Option<ActionsJobReconciliation>>,
    inventories: VecDeque<Option<Vec<OwnedDockerResource>>>,
    observed_binding: JournalDockerDaemonBinding,
    inventory_must_see_receipt: bool,
    rest_calls: usize,
    inventory_calls: usize,
    rest_pends: bool,
    rest_dropped: Arc<AtomicBool>,
}

impl RecoveryServices for ScriptedRecovery<'_> {
    fn completed_rest(
        &mut self,
        row: IntentRow,
        _deadline: tokio::time::Instant,
    ) -> RecoveryFuture<'_, Option<ActionsJobReconciliation>> {
        self.rest_calls += 1;
        let has_identity = row.github_runner_id.as_deref() == Some("88")
            && row.runner_name.as_deref() == Some(RUNNER_NAME)
            && row.observed_job_id.as_deref() == Some(SCALE_SET_JOB_ID)
            && row.observed_workflow_run_id == Some(45);
        assert!(
            has_identity,
            "REST lookup must use persisted Started identity"
        );
        if self.rest_pends {
            let dropped = Arc::clone(&self.rest_dropped);
            return Box::pin(async move {
                struct DropSignal(Arc<AtomicBool>);
                impl Drop for DropSignal {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::Release);
                    }
                }
                let _signal = DropSignal(dropped);
                std::future::pending::<Option<ActionsJobReconciliation>>().await
            });
        }
        let result = self.rest.pop_front().unwrap_or(None);
        Box::pin(async move { result })
    }

    fn complete_inventory(
        &mut self,
        binding: JournalDockerDaemonBinding,
        _deadline: tokio::time::Instant,
    ) -> RecoveryFuture<'_, Option<Vec<OwnedDockerResource>>> {
        self.inventory_calls += 1;
        assert_eq!(binding, self.observed_binding);
        let inventory = self.inventories.pop_front().unwrap_or(None);
        let journal = self.journal;
        let must_see_receipt = self.inventory_must_see_receipt;
        Box::pin(async move {
            if must_see_receipt {
                let rows = journal.rows().await.expect("receipt row is readable");
                assert!(
                    rows.iter().any(|row| {
                        row.kind == "launch"
                            && row.observed_actions_attempt == Some(2)
                            && row.observed_actions_job_id == Some(9001)
                            && row.remote_terminal
                            && !row.cleanup_proven
                    }),
                    "completed REST evidence must commit before inventory/adoption"
                );
            }
            inventory
        })
    }
}

#[tokio::test]
async fn lost_rest_response_keeps_the_same_generation_unbound_and_occupied() -> Result<(), String> {
    let (_scratch, _path, journal, launch_id, binding) = launch_fixture("lost-rest").await?;
    let mut services = scripted(&journal, binding.clone(), [None], [], false);
    let mut budget = RecoveryBudget::default();
    let pass = run_scripted_pass(&journal, &binding, &mut services, &mut budget)
        .await
        .ok_or_else(|| "lost response aborted the durable journal scan".to_owned())?;

    assert_eq!(pass, Vec::<i64>::new());
    assert_eq!(services.rest_calls, 1);
    assert_eq!(services.inventory_calls, 0);
    let row = launch_row(&journal, launch_id).await?;
    assert!(!row.remote_terminal);
    assert_eq!(row.observed_actions_job_id, None);
    assert!(!row.cleanup_proven);
    assert!(holds(&row));
    assert_eq!(
        journal
            .launch_daemon_binding(launch_id)
            .await
            .map_err(|error| error.to_string())?,
        None
    );
    Ok(())
}

#[tokio::test]
async fn completed_receipt_survives_restart_until_exact_inventory_adoption() -> Result<(), String> {
    let (_scratch, path, journal, launch_id, binding) = launch_fixture("rest-restart").await?;
    let mut unavailable = scripted(
        &journal,
        binding.clone(),
        [Some(completed_reconciliation())],
        [None],
        true,
    );
    let mut budget = RecoveryBudget::default();
    let first = run_scripted_pass(&journal, &binding, &mut unavailable, &mut budget)
        .await
        .ok_or_else(|| "completed REST retry aborted before inventory".to_owned())?;
    assert_eq!(first, Vec::<i64>::new());
    assert_eq!(unavailable.rest_calls, 1);
    assert_eq!(unavailable.inventory_calls, 1);
    assert_still_held_unbound(&journal, launch_id).await?;
    drop(journal);

    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let row = launch_row(&reopened, launch_id).await?;
    let mut available = scripted(
        &reopened,
        binding.clone(),
        [],
        [Some(inventory_fixtures::complete_inventory(&row)?)],
        true,
    );
    let mut retry_budget = RecoveryBudget::default();
    let second = run_scripted_pass(&reopened, &binding, &mut available, &mut retry_budget)
        .await
        .ok_or_else(|| "restart reconciliation failed".to_owned())?;

    assert_eq!(second, vec![launch_id]);
    assert_eq!(available.rest_calls, 0);
    assert_eq!(available.inventory_calls, 1);
    assert_eq!(
        reopened
            .launch_daemon_binding(launch_id)
            .await
            .map_err(|error| error.to_string())?,
        Some(binding)
    );
    let row = launch_row(&reopened, launch_id).await?;
    assert!(
        !row.cleanup_proven,
        "adoption is not physical cleanup proof"
    );
    assert!(
        holds(&row),
        "capacity stays occupied until ordinary cleanup"
    );
    Ok(())
}

#[tokio::test]
async fn shutdown_drops_pending_reconciliation_without_inventory_or_binding() -> Result<(), String>
{
    let (_scratch, _path, journal, launch_id, binding) =
        launch_fixture("recovery-shutdown").await?;
    let mut services = scripted(&journal, binding.clone(), [], [], false);
    services.rest_pends = true;
    let mut budget = RecoveryBudget::default();
    let (sender, mut receiver) = tokio::sync::watch::channel(None);
    let mut cutoff = None;
    let mut shutdown = ShutdownGate {
        receiver: &mut receiver,
        cutoff: &mut cutoff,
    };
    let expiry = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        sender
            .send(Some(Instant::now()))
            .expect("recovery receiver remains active");
    });
    let result = run_recovery_pass(
        &journal,
        &binding,
        &mut services,
        &mut budget,
        Duration::from_secs(5),
        &mut shutdown,
    )
    .await;
    expiry.await.map_err(|error| error.to_string())?;

    assert!(
        result.is_none(),
        "expired recovery returns no cleanup authority"
    );
    assert!(services.rest_dropped.load(Ordering::Acquire));
    assert_eq!(services.rest_calls, 1);
    assert_eq!(services.inventory_calls, 0);
    assert_still_unstarted_and_held(&journal, launch_id).await?;
    Ok(())
}

#[test]
fn recovery_budget_has_a_finite_window_attempt_cap_and_backoff() {
    let start = Instant::now();
    let mut budget = RecoveryBudget::default();
    assert!(budget.next_deadline(start, None).is_some());
    assert!(
        budget
            .next_deadline(start + Duration::from_millis(100), None)
            .is_none()
    );
    let mut now = start + Duration::from_millis(250);
    for _ in 1..8 {
        assert!(budget.next_deadline(now, None).is_some());
        now += Duration::from_secs(4);
    }
    assert!(budget.next_deadline(now, None).is_none());
    budget.reset();
    let expired = start
        .checked_sub(Duration::from_millis(1))
        .expect("Instant has sufficient earlier range");
    assert!(budget.next_deadline(start, Some(expired)).is_none());
}
