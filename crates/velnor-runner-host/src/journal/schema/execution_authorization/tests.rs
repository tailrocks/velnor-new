// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "tests/fixture.rs"]
mod fixture;

use crate::journal::Journal;

use fixture::{
    DIGEST, OTHER_DIGEST, OTHER_SOURCE_JSON, PROOF_TABLE, PUBLISHED_CHALLENGE, SOURCE_JSON,
    TestResult, challenge_file_inode, challenge_state, clear_challenge_file,
    downgrade_guards_to_v52, downgrade_to_v52, insert_proof, journal_proof_guards,
    proof_guards_database, publish_challenge, record_source, remove_source, scratch_database,
    source_digest,
};

#[tokio::test]
async fn a_recorded_execution_source_survives_every_update() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let journal = Journal::open(&dir.path().join("journal.db")).await?;
    let connection = proof_guards_database(&dir, "proof.db", &journal).await?;
    insert_proof(&connection, 1, PUBLISHED_CHALLENGE, "requested").await?;
    publish_challenge(&connection, 1).await?;
    record_source(&connection, 1, DIGEST, SOURCE_JSON).await?;

    let removed = remove_source(&connection, 1).await;
    assert!(removed.is_err(), "a recorded source was removed");

    let replaced = connection
        .execute(
            &format!(
                "UPDATE {PROOF_TABLE} SET source_digest = ?1, source_json = ?2 WHERE intent_id = ?3"
            ),
            (OTHER_DIGEST, OTHER_SOURCE_JSON, 1),
        )
        .await;
    assert!(replaced.is_err(), "a recorded source was replaced");

    assert_eq!(
        source_digest(&connection, 1).await?,
        Some(DIGEST.to_owned())
    );
    Ok(())
}

#[tokio::test]
async fn an_execution_source_is_recorded_once_for_a_published_challenge() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let journal = Journal::open(&dir.path().join("journal.db")).await?;
    let connection = proof_guards_database(&dir, "proof.db", &journal).await?;
    insert_proof(&connection, 1, PUBLISHED_CHALLENGE, "requested").await?;
    publish_challenge(&connection, 1).await?;

    record_source(&connection, 1, DIGEST, SOURCE_JSON).await?;

    assert_eq!(
        source_digest(&connection, 1).await?,
        Some(DIGEST.to_owned())
    );
    Ok(())
}

#[tokio::test]
async fn a_published_challenge_keeps_its_file_identity() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let journal = Journal::open(&dir.path().join("journal.db")).await?;
    let connection = proof_guards_database(&dir, "proof.db", &journal).await?;
    insert_proof(&connection, 1, PUBLISHED_CHALLENGE, "requested").await?;
    publish_challenge(&connection, 1).await?;

    let cleared = clear_challenge_file(&connection, 1).await;
    assert!(
        cleared.is_err(),
        "a published challenge lost its file identity"
    );
    assert_eq!(challenge_file_inode(&connection, 1).await?, Some(11));

    connection
        .execute(
            &format!("UPDATE {PROOF_TABLE} SET challenge_state = 'abandoned' WHERE intent_id = ?1"),
            (1,),
        )
        .await?;
    assert_eq!(
        challenge_state(&connection, 1).await?,
        Some("abandoned".to_owned())
    );
    Ok(())
}

#[tokio::test]
async fn a_v52_journal_migrates_to_the_null_safe_proof_guards() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    downgrade_to_v52(&journal).await?;

    let (proof_table, guards) = journal_proof_guards(&journal).await?;
    let connection = scratch_database(&dir, "v52.db", &proof_table, &guards).await?;
    insert_proof(&connection, 1, PUBLISHED_CHALLENGE, "requested").await?;
    publish_challenge(&connection, 1).await?;
    record_source(&connection, 1, DIGEST, SOURCE_JSON).await?;
    remove_source(&connection, 1).await?;
    assert_eq!(source_digest(&connection, 1).await?, None);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let (proof_table, guards) = journal_proof_guards(&journal).await?;
    let connection = scratch_database(&dir, "v53.db", &proof_table, &guards).await?;
    insert_proof(&connection, 1, PUBLISHED_CHALLENGE, "requested").await?;
    publish_challenge(&connection, 1).await?;
    record_source(&connection, 1, DIGEST, SOURCE_JSON).await?;

    let removed = remove_source(&connection, 1).await;
    assert!(
        removed.is_err(),
        "a migrated journal kept a removable source"
    );
    Ok(())
}

#[tokio::test]
async fn a_current_journal_refuses_the_version_52_proof_guards() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("journal.db");
    let journal = Journal::open(&path).await?;
    downgrade_guards_to_v52(&journal).await?;
    drop(journal);

    let reopened = Journal::open(&path).await;
    assert!(
        reopened.is_err(),
        "a journal with the version 52 proof guards was accepted"
    );
    Ok(())
}
