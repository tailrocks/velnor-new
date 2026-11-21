// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};

use super::super::controller::Controller;
use super::{CRASHED, Crash, Prepared, TestResult, db, prepare};

#[derive(Clone, Copy, Debug)]
enum Corruption {
    Digest,
    Canonicality,
    RowIdentity,
    Labels,
}

impl Corruption {
    fn apply(self, json: &str, digest: &str) -> (String, String) {
        let changed = match self {
            Self::Digest => {
                let mut changed = digest.to_owned();
                changed.replace_range(..1, if digest.starts_with('0') { "1" } else { "0" });
                return (json.to_owned(), changed);
            }
            Self::Canonicality => format!("{json} "),
            Self::RowIdentity => replace_once(
                json,
                &format!("\"docker_id\":\"{}\"", CRASHED.container_id("dind")),
                "\"docker_id\":\"another-valid-container-id\"",
            ),
            Self::Labels => replace_once(
                json,
                "\"io.velnor.managed\":\"true\"",
                "\"io.velnor.managed\":\"false\"",
            ),
        };
        let digest = crate::digest_hex::lowercase_hex(&Sha256::digest(changed.as_bytes()));
        (changed, digest)
    }
}

fn replace_once(source: &str, old: &str, new: &str) -> String {
    assert_eq!(
        source.matches(old).count(),
        1,
        "tampering must change exactly one field"
    );
    source.replacen(old, new, 1)
}

async fn receipt_after_delete() -> TestResult<Prepared> {
    let case = prepare(Crash::DindCreate, false).await?.restart().await?;
    let id = CRASHED.container_id("dind");
    let deletion = case.fixture.delay_delete("containers", &id);
    let mut controller = Controller::new(&case.scope, &case.engine)?;
    let mut cleanup = Box::pin(controller.turn(&case.journal, &case.docker, &case.engine));
    tokio::select! {
        result = &mut cleanup => return Err(format!("cleanup completed before receipt DELETE barrier: {result:?}").into()),
        () = deletion.reached() => {},
    }
    db::target(
        &case.journal,
        "container",
        "dind",
        &format!("w{}-dind", CRASHED.nonce()),
        &id,
    )
    .await?;
    assert!(!case.fixture.objects(CRASHED).contains(&id));
    drop(cleanup);
    drop(controller);
    deletion.release();
    let case = case.restart().await?;
    db::target(
        &case.journal,
        "container",
        "dind",
        &format!("w{}-dind", CRASHED.nonce()),
        &id,
    )
    .await?;
    Ok(case)
}

async fn rejects_named(corruption: Corruption) -> TestResult<()> {
    rejects(corruption)
        .await
        .map_err(|error| format!("{corruption:?}: {error}").into())
}

#[tokio::test(flavor = "multi_thread")]
async fn reopening_rejects_corrupt_creation_receipts_without_rewriting_them_digest()
-> TestResult<()> {
    rejects_named(Corruption::Digest).await
}

#[tokio::test(flavor = "multi_thread")]
async fn reopening_rejects_corrupt_creation_receipts_without_rewriting_them_canonicality()
-> TestResult<()> {
    rejects_named(Corruption::Canonicality).await
}

#[tokio::test(flavor = "multi_thread")]
async fn reopening_rejects_corrupt_creation_receipts_without_rewriting_them_row_identity()
-> TestResult<()> {
    rejects_named(Corruption::RowIdentity).await
}

#[tokio::test(flavor = "multi_thread")]
async fn reopening_rejects_corrupt_creation_receipts_without_rewriting_them_labels()
-> TestResult<()> {
    rejects_named(Corruption::Labels).await
}

async fn rejects(corruption: Corruption) -> TestResult<()> {
    let case = receipt_after_delete().await?;
    let connection = case.journal.connection().await?;
    let original = receipt(&connection).await?;
    let value: serde_json::Value = serde_json::from_str(&original)?;
    let json = value
        .get(0)
        .and_then(serde_json::Value::as_str)
        .ok_or("receipt JSON")?;
    let digest = value
        .get(1)
        .and_then(serde_json::Value::as_str)
        .ok_or("receipt SHA")?;
    let (json, digest) = corruption.apply(json, digest);
    let ddl = scalar(&connection, "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name='unbound_creation_target_no_update'").await?;
    connection
        .execute("DROP TRIGGER unbound_creation_target_no_update", ())
        .await?;
    let changed = connection.execute("UPDATE capacity_reservation_unbound_creation_targets SET observation_json=?1,observation_sha256=?2 WHERE reservation_id=2 AND resource_kind='container' AND role='dind'", (json, digest)).await;
    connection.execute(&ddl, ()).await?;
    assert_eq!(changed?, 1);
    assert_eq!(scalar(&connection, "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name='unbound_creation_target_no_update'").await?, ddl);
    let invalid = receipt(&connection).await?;
    assert_ne!(original, invalid);
    let before = metadata(&connection).await?;
    let Prepared {
        directory: _directory,
        fixture: _fixture,
        docker: _docker,
        path,
        journal,
        scope,
        engine: _engine,
        policy: _policy,
    } = case;
    drop(connection);
    drop(scope);
    drop(journal);
    assert!(
        crate::journal::Journal::open(&path).await.is_err(),
        "{corruption:?} receipt was accepted"
    );
    let database = turso::Builder::new_local(path.to_str().ok_or("journal path")?)
        .build()
        .await?;
    let connection = database.connect()?;
    assert_eq!(
        receipt(&connection).await?,
        invalid,
        "failed open must preserve the invalid receipt"
    );
    assert_eq!(
        metadata(&connection).await?,
        before,
        "failed open must preserve schema version and journal revision"
    );
    Ok(())
}

async fn receipt(connection: &turso::Connection) -> TestResult<String> {
    scalar(connection, "SELECT json_array(observation_json,observation_sha256,reservation_id,resource_kind,role,docker_name,docker_id,connection_id,operation_id,engine_id,docker_root_digest,observation_claim_generation,observed_at_unix_ms) FROM capacity_reservation_unbound_creation_targets WHERE reservation_id=2 AND resource_kind='container' AND role='dind'").await
}

async fn metadata(connection: &turso::Connection) -> TestResult<(String, String)> {
    Ok((
        scalar(connection, "PRAGMA user_version").await?,
        scalar(
            connection,
            "SELECT CAST(revision AS TEXT) FROM journal_meta WHERE singleton=1",
        )
        .await?,
    ))
}

async fn scalar(connection: &turso::Connection, sql: &str) -> TestResult<String> {
    let mut rows = connection.query(sql, ()).await?;
    let row = rows.next().await?.ok_or("receipt journal row")?;
    let value = match row.get::<String>(0) {
        Ok(value) => value,
        Err(_) => row.get::<i64>(0)?.to_string(),
    };
    assert!(rows.next().await?.is_none());
    Ok(value)
}
