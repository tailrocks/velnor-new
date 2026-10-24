//! Attached engine-lineage behavior for scoped journal transactions.

use crate::connect::SetOwnership;
use crate::daemon_lock::EngineLineageGuard;
use crate::error::HostError;
use crate::journal::{Journal, Outcome};

use super::{Scratch, connection};

#[tokio::test]
async fn scoped_transactions_keep_lineage_attached_through_clone_drop_and_restart()
-> Result<(), HostError> {
    let scratch = Scratch::new("lineage-restart")?;
    let path = scratch.file();
    let engine_id = format!("journal-test-{}", uuid::Uuid::new_v4().simple());
    let binding = connection("lineage", 303, "Velnor/lineage-test");
    let journal = Journal::open(&path).await?;
    journal.bind_engine(&engine_id).await?;
    let instance = journal.instance_id().await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(&engine_id, scratch.root())?;
    guard.verify_lineage(journal.path(), &instance, journal.revision().await?)?;
    journal.attach_lineage_guard(guard.clone())?;

    let before_scope = journal.revision().await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(journal.revision().await?, before_scope + 1);
    scope
        .bind_scale_set(10_303, "integration", SetOwnership::Adopted)
        .await?;
    let clone = journal.clone();
    drop(journal);
    let replacement_guard = EngineLineageGuard::acquire_at_for_tests(
        &format!("wrong-{}", uuid::Uuid::new_v4().simple()),
        scratch.root(),
    )?;
    clone.attach_lineage_guard(replacement_guard)?;

    let intent = scope.begin("session", "lineage-session").await?;
    scope.finish(intent, Outcome::Done).await?;
    scope.record_cleanup(intent).await?;
    let committed_revision = clone.revision().await?;
    guard.verify_lineage(clone.path(), &instance, committed_revision)?;

    // Simulate process death after a one-revision database commit and before anchor sync.
    let connection = clone.connection().await?;
    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE connection_scopes SET retired = 1 WHERE connection_id = ?1",
            [binding.connection_id.as_str()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("COMMIT", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    let crash_revision = clone.revision().await?;
    drop(scope);
    drop(clone);
    drop(guard);

    let restarted_guard = EngineLineageGuard::acquire_at_for_tests(&engine_id, scratch.root())?;
    restarted_guard.verify_lineage(&path, &instance, crash_revision)?;
    let restarted = Journal::open(&path).await?;
    restarted.attach_lineage_guard(restarted_guard)?;
    restarted.sync_lineage().await?;
    assert!(restarted.reactivate_if_drained(&binding).await?);
    Ok(())
}
