//! Attached engine-lineage tests for the final cleanup transaction.

use crate::daemon_lock::test_engine_lineage_guard;
use crate::{HostError, Journal};

#[tokio::test]
async fn upgraded_trigger_records_terminal_cleanup_as_one_revision() -> Result<(), String> {
    let (scratch, journal) = super::open("completion-trigger-upgrade").await?;
    let engine = "docker-engine-test";
    journal
        .establish_engine_lineage(engine, lineage_guard(engine, &scratch.file())?)
        .await
        .map_err(|error| error.to_string())?;
    let id = completed_launch(&journal, 151).await?;
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected completion claim".to_owned())?;
    if !journal
        .mark_completion_worker_cleanup_proven_at(id, claim.generation, 101)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected worker cleanup proof".to_owned());
    }
    let connection = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("DROP TRIGGER completion_cleanup_revision_delete", ())
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TRIGGER completion_cleanup_revision_delete AFTER DELETE ON completion_cleanup BEGIN UPDATE journal_meta SET revision = revision + 1 WHERE singleton = 1; END",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    drop(journal);

    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .establish_engine_lineage(engine, lineage_guard(engine, &scratch.file())?)
        .await
        .map_err(|error| error.to_string())?;
    let before = journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .record_completion_cleanup_at(id, claim.generation, 102)
            .await
            .map_err(|error| error.to_string())?
    );
    assert_eq!(
        journal
            .revision()
            .await
            .map_err(|error| error.to_string())?,
        before + 1,
        "the bootstrap must replace an older unconditional delete trigger"
    );
    drop(journal);

    let reopened = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    reopened
        .establish_engine_lineage(engine, lineage_guard(engine, &scratch.file())?)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(reopened.occupied_launches().await, Ok(0));
    Ok(())
}

#[tokio::test]
async fn attached_lineage_recovers_atomic_cleanup_after_commit_before_anchor() -> Result<(), String>
{
    let (scratch, journal) = super::open("completion-anchor-crash").await?;
    let engine = "docker-engine-test";
    journal
        .establish_engine_lineage(engine, lineage_guard(engine, &scratch.file())?)
        .await
        .map_err(|error| error.to_string())?;
    let id = completed_launch(&journal, 152).await?;
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected completion claim".to_owned())?;
    if !journal
        .mark_completion_worker_cleanup_proven_at(id, claim.generation, 101)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected worker cleanup proof".to_owned());
    }
    let before = journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    let connection = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("BEGIN IMMEDIATE", ())
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE intents SET cleanup_proven = 1 WHERE id = ?1 AND kind = 'launch' AND runner_completed = 1 AND worker_cleanup_proven = 1 AND cleanup_proven = 0 AND EXISTS (SELECT 1 FROM completion_cleanup AS c WHERE c.intent_id = intents.id AND c.claim_generation = ?2 AND c.lease_until > ?3)",
            (id, claim.generation, 102),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "DELETE FROM completion_cleanup WHERE intent_id = ?1 AND claim_generation = ?2 AND lease_until > ?3",
            (id, claim.generation, 102),
        )
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("COMMIT", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(connection);
    assert_eq!(
        journal
            .revision()
            .await
            .map_err(|error| error.to_string())?,
        before + 1,
        "one atomic terminal transition must have one recoverable revision"
    );
    drop(journal);

    let reopened = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    reopened
        .establish_engine_lineage(engine, lineage_guard(engine, &scratch.file())?)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(reopened.occupied_launches().await, Ok(0));
    Ok(())
}

#[tokio::test]
async fn deleting_a_nonterminal_claim_still_advances_lineage_revision() -> Result<(), String> {
    let (scratch, journal) = super::open("completion-nonterminal-delete").await?;
    let engine = "docker-engine-test";
    journal
        .establish_engine_lineage(engine, lineage_guard(engine, &scratch.file())?)
        .await
        .map_err(|error| error.to_string())?;
    let id = completed_launch(&journal, 153).await?;
    let claim = journal
        .claim_completion_cleanup_at(id, 100, 110)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "expected completion claim".to_owned())?;
    let before = journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    let connection = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "DELETE FROM completion_cleanup WHERE intent_id = ?1 AND claim_generation = ?2",
            (id, claim.generation),
        )
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .revision()
            .await
            .map_err(|error| error.to_string())?,
        before + 1,
        "unproven claim deletion must keep its revision increment"
    );
    journal
        .sync_lineage()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(journal.occupied_launches().await, Ok(1));
    connection
        .execute(
            "INSERT INTO completion_cleanup (intent_id, attempts, claim_generation, retry_after, lease_until) VALUES (999, 0, 1, 0, 0)",
            (),
        )
        .await
        .map_err(|error| error.to_string())?;
    journal
        .sync_lineage()
        .await
        .map_err(|error| error.to_string())?;
    let before_orphan_delete = journal
        .revision()
        .await
        .map_err(|error| error.to_string())?;
    connection
        .execute("DELETE FROM completion_cleanup WHERE intent_id = 999", ())
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .revision()
            .await
            .map_err(|error| error.to_string())?,
        before_orphan_delete + 1,
        "deleting an orphan claim must not be hidden as terminal cleanup"
    );
    journal
        .sync_lineage()
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn completed_launch(journal: &Journal, request_id: i64) -> Result<i64, String> {
    let id = super::launch(journal, request_id).await?;
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_completed(
            1,
            request_id,
            1_000 + request_id,
            &format!("v{}", identity.launch_id()),
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(id)
}

fn lineage_guard(
    engine: &str,
    journal_path: &std::path::Path,
) -> Result<crate::daemon_lock::EngineLineageGuard, String> {
    let root = journal_path
        .parent()
        .ok_or_else(|| "journal has no parent".to_owned())?;
    test_engine_lineage_guard(engine, root).map_err(|error: HostError| error.to_string())
}
