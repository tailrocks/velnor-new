//! Durable legacy-launch adoption tests over Started, REST, and inventory evidence.

use std::path::Path;

use velnor_runner_github::InnerKind;

use crate::Journal;
use crate::journal::{JournalDockerDaemonBinding, LegacyLaunchAdoption};
use crate::reconcile::IntentRow;

use super::Scratch;
use super::actions_reconciliation_tests::{
    lifecycle_event, prepared_launch, reconciliation, started_launch,
};

#[tokio::test]
async fn started_rest_inventory_adoption_is_atomic_durable_and_idempotent() -> Result<(), String> {
    let scratch = Scratch::new("legacy-adoption-durable").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch_id = started_launch(&journal).await?;
    let completed = reconciliation(9011, Some("failure"));
    journal
        .record_actions_job_reconciliation(launch_id, &completed)
        .await
        .map_err(|error| error.to_string())?;
    let expected = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "launch row disappeared before adoption".to_owned())?;
    assert!(started_marker(&path, launch_id).await?);
    let binding = JournalDockerDaemonBinding::new("/run/docker.sock", "logical-engine-a")
        .map_err(|error| error.to_string())?;
    assert_adoption_before_reopen(&journal, launch_id, &expected, &binding).await?;
    drop(journal);
    assert_adoption_after_reopen(&path, launch_id, &expected, &binding).await
}

async fn assert_adoption_before_reopen(
    journal: &Journal,
    launch_id: i64,
    expected: &IntentRow,
    binding: &JournalDockerDaemonBinding,
) -> Result<(), String> {
    assert!(
        journal
            .launch_daemon_binding(launch_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    assert_eq!(
        journal
            .adopt_legacy_launch_on_engine(expected, binding)
            .await
            .map_err(|error| error.to_string())?,
        LegacyLaunchAdoption::Adopted
    );
    assert_eq!(
        journal
            .adopt_legacy_launch_on_engine(expected, binding)
            .await
            .map_err(|error| error.to_string())?,
        LegacyLaunchAdoption::AlreadyAdopted
    );
    let unbound = journal
        .unbound_started_launches()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(unbound, Vec::<IntentRow>::new());
    Ok(())
}

async fn assert_adoption_after_reopen(
    path: &Path,
    launch_id: i64,
    expected: &IntentRow,
    binding: &JournalDockerDaemonBinding,
) -> Result<(), String> {
    let reopened = Journal::open(path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened
            .launch_daemon_binding(launch_id)
            .await
            .map_err(|error| error.to_string())?,
        Some(binding.clone())
    );
    let row = reopened
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "launch row disappeared after reopen".to_owned())?;
    assert_eq!(&row, expected);
    assert!(row.remote_terminal);
    assert!(!row.cleanup_proven);
    let unbound = reopened
        .unbound_started_launches()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(unbound, Vec::<IntentRow>::new());
    assert_eq!(
        reopened
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        1,
        "binding adoption must not release the held launch slot"
    );
    assert_eq!(
        reopened
            .adopt_legacy_launch_on_engine(&row, binding)
            .await
            .map_err(|error| error.to_string())?,
        LegacyLaunchAdoption::AlreadyAdopted
    );
    Ok(())
}

#[tokio::test]
async fn late_duplicate_started_after_terminal_rest_evidence_is_durable_once() -> Result<(), String>
{
    let scratch = Scratch::new("late-started-after-rest").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch_id = prepared_launch(&journal).await?;
    assert!(
        journal
            .observe_runner_event(&lifecycle_event(InnerKind::Completed))
            .await
            .map_err(|error| error.to_string())?
    );
    journal
        .record_actions_job_reconciliation(launch_id, &reconciliation(9014, Some("failure")))
        .await
        .map_err(|error| error.to_string())?;
    assert!(!started_marker(&path, launch_id).await?);
    let before_started = journal
        .unbound_started_launches()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(before_started, Vec::<IntentRow>::new());

    for _ in 0..2 {
        assert!(
            journal
                .observe_runner_event(&lifecycle_event(InnerKind::Started))
                .await
                .map_err(|error| error.to_string())?
        );
    }
    assert_eq!(started_marker_count(&path, launch_id).await?, 1);
    let rows = journal
        .unbound_started_launches()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, launch_id);
    assert!(rows[0].remote_terminal);
    assert_eq!(rows[0].observed_actions_job_id, Some(9014));
    assert!(!rows[0].cleanup_proven);
    Ok(())
}

#[tokio::test]
async fn stale_row_and_failed_provenance_insert_never_leave_partial_binding() -> Result<(), String>
{
    let scratch = Scratch::new("legacy-adoption-rollback").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch_id = started_launch(&journal).await?;
    let stale = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "launch row disappeared before REST reconciliation".to_owned())?;
    journal
        .record_actions_job_reconciliation(launch_id, &reconciliation(9012, Some("failure")))
        .await
        .map_err(|error| error.to_string())?;
    let binding = JournalDockerDaemonBinding::new("/run/docker.sock", "logical-engine-a")
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .adopt_legacy_launch_on_engine(&stale, &binding)
            .await
            .is_err()
    );
    assert!(
        journal
            .launch_daemon_binding(launch_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );

    let current = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "launch row disappeared before rollback test".to_owned())?;
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE linux_launch_daemon_adoptions", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(conn);
    drop(database);

    assert!(
        journal
            .adopt_legacy_launch_on_engine(&current, &binding)
            .await
            .is_err()
    );
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
        1,
        "a failed provenance insert must roll back the binding and preserve occupancy"
    );
    Ok(())
}

#[tokio::test]
async fn v12_upgrade_does_not_backfill_started_or_adopt_legacy_rows() -> Result<(), String> {
    let scratch = Scratch::new("legacy-adoption-v12-upgrade").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let launch_id = started_launch(&journal).await?;
    journal
        .record_actions_job_reconciliation(launch_id, &reconciliation(9013, Some("failure")))
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);

    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE linux_launch_daemon_adoptions", ())
        .await
        .map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE linux_launch_started_observations", ())
        .await
        .map_err(|error| error.to_string())?;
    conn.execute("PRAGMA user_version = 12", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(conn);
    drop(database);

    let upgraded = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let row = upgraded
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == launch_id)
        .ok_or_else(|| "legacy launch disappeared during V12 upgrade".to_owned())?;
    assert!(
        !started_marker(&path, launch_id).await?,
        "migration cannot invent Started proof"
    );
    assert!(
        row.remote_terminal,
        "existing REST completion remains durable"
    );
    assert!(!row.cleanup_proven);
    assert!(
        upgraded
            .launch_daemon_binding(launch_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    assert_eq!(
        upgraded
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        1
    );
    let binding = JournalDockerDaemonBinding::new("/run/docker.sock", "logical-engine-a")
        .map_err(|error| error.to_string())?;
    assert!(
        upgraded
            .adopt_legacy_launch_on_engine(&row, &binding)
            .await
            .is_err()
    );
    Ok(())
}

async fn started_marker(path: &Path, launch_id: i64) -> Result<bool, String> {
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    let mut rows = conn
        .query(
            "SELECT EXISTS (SELECT 1 FROM linux_launch_started_observations WHERE launch_id = ?1)",
            [launch_id],
        )
        .await
        .map_err(|error| error.to_string())?;
    let present = rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Started marker query returned no row".to_owned())?
        .get::<i64>(0)
        .map_err(|error| error.to_string())?;
    match present {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("Started marker query returned invalid boolean".to_owned()),
    }
}

async fn started_marker_count(path: &Path, launch_id: i64) -> Result<i64, String> {
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    let mut rows = conn
        .query(
            "SELECT COUNT(*) FROM linux_launch_started_observations WHERE launch_id = ?1",
            [launch_id],
        )
        .await
        .map_err(|error| error.to_string())?;
    rows.next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Started marker count query returned no row".to_owned())?
        .get::<i64>(0)
        .map_err(|error| error.to_string())
}
