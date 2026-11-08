// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, io::Read, os::unix::fs::PermissionsExt, path::Path};
use velnor_runner_host::{IntentState, Journal};

type TestResult = Result<(), Box<dyn Error>>;

async fn connection(path: &Path) -> Result<turso::Connection, Box<dyn Error>> {
    Ok(
        turso::Builder::new_local(path.to_str().ok_or("non-UTF8 path")?)
            .build()
            .await?
            .connect()?,
    )
}

async fn new_journal(path: &Path) -> TestResult {
    drop(Journal::open(path).await?);
    Ok(())
}

async fn insert_registry(
    conn: &turso::Connection,
    engine: &str,
    state: &str,
    runtime: Option<String>,
    receipt: Option<String>,
) -> TestResult {
    conn.execute(
        "INSERT INTO image_import_registry (engine_id,docker_root_digest,daemon_architecture,source_sha,release_manifest_sha256,role,registry_digest,config_digest,attempt,state,runtime_id,receipt) VALUES (?1,?2,'x86_64',?3,?4,'runner',?5,?6,0,?7,?8,?9)",
        (engine.to_owned(), "a".repeat(64), "b".repeat(40), "c".repeat(64), format!("sha256:{}", "d".repeat(64)), format!("sha256:{}", "e".repeat(64)), state.to_owned(), runtime, receipt),
    ).await?;
    Ok(())
}

async fn insert_archive(conn: &turso::Connection, engine: &str, state: &str) -> TestResult {
    conn.execute("INSERT INTO image_imports (engine_id,docker_root_digest,daemon_architecture,source_sha,archive_sha256,index_digest,platform,manifest_digest,config_digest,attempt,state,image_id) VALUES (?1,?2,'x86_64',?3,?4,?5,'linux/amd64',?6,?7,?8,?9,NULL)", (engine.to_owned(), "a".repeat(64), "b".repeat(40), "c".repeat(64), "d".repeat(64), format!("sha256:{}", "d".repeat(64)), format!("sha256:{}", "e".repeat(64)), i64::from(state == "requested"), state.to_owned())).await?;
    Ok(())
}

#[tokio::test]
async fn unresolved_imports_reserve_each_engine_across_both_tables() -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("journal.db");
    new_journal(&path).await?;
    let conn = connection(&path).await?;
    insert_archive(&conn, "engine-a", "prepared").await?;
    assert!(
        insert_archive(&conn, "engine-a", "requested")
            .await
            .is_err()
    );
    assert!(
        insert_registry(&conn, "engine-a", "prepared", None, None)
            .await
            .is_err()
    );
    insert_registry(&conn, "engine-b", "prepared", None, None).await?;
    assert!(conn.execute("INSERT INTO image_import_registry (engine_id,docker_root_digest,daemon_architecture,source_sha,release_manifest_sha256,role,registry_digest,config_digest,attempt,state,runtime_id,receipt) SELECT engine_id,docker_root_digest,daemon_architecture,source_sha,release_manifest_sha256,role,registry_digest,config_digest,attempt + 1,state,runtime_id,receipt FROM image_import_registry WHERE engine_id = 'engine-b'", ()).await.is_err());
    assert!(insert_archive(&conn, "engine-b", "prepared").await.is_err());
    insert_archive(&conn, "engine-c", "aborted").await?;
    assert!(conn.execute("UPDATE image_imports SET state = 'requested', engine_id = 'engine-b' WHERE engine_id = 'engine-c'", ()).await.is_err());
    assert!(
        conn.execute(
            "UPDATE image_import_registry SET engine_id = 'engine-a' WHERE engine_id = 'engine-b'",
            ()
        )
        .await
        .is_err()
    );
    conn.execute(
        "UPDATE image_import_registry SET state = 'requested' WHERE engine_id = 'engine-b'",
        (),
    )
    .await?;
    let digest = format!("sha256:{}", "e".repeat(64));
    let receipt =
        format!("{{\"store\":\"classic\",\"runtime_id\":\"{digest}\",\"manifest_size\":1}}");
    conn.execute("UPDATE image_import_registry SET state = 'completed', runtime_id = ?1, receipt = ?2 WHERE engine_id = 'engine-b'", (digest, receipt)).await?;
    insert_archive(&conn, "engine-b", "prepared").await?;
    conn.execute(
        "UPDATE image_imports SET state = 'requested' WHERE engine_id = 'engine-a'",
        (),
    )
    .await?;
    conn.execute(
        "UPDATE image_imports SET state = 'aborted' WHERE engine_id = 'engine-a'",
        (),
    )
    .await?;
    insert_registry(&conn, "engine-a", "prepared", None, None).await?;
    drop(conn);
    assert!(Journal::open(&path).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn competing_connections_commit_only_one_unresolved_import() -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("journal.db");
    new_journal(&path).await?;
    let archive = connection(&path).await?;
    let registry = connection(&path).await?;
    let barrier = tokio::sync::Barrier::new(2);
    let (first, second) = tokio::join!(
        async {
            barrier.wait().await;
            insert_archive(&archive, "engine-a", "prepared").await
        },
        async {
            barrier.wait().await;
            insert_registry(&registry, "engine-a", "prepared", None, None).await
        },
    );
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "exactly one reservation must commit"
    );
    let mut rows = archive.query("SELECT (SELECT count(*) FROM image_imports WHERE engine_id = 'engine-a' AND state IN ('prepared','requested','uncertain')) + (SELECT count(*) FROM image_import_registry WHERE engine_id = 'engine-a' AND state IN ('prepared','requested','uncertain'))", ()).await?;
    assert_eq!(rows.next().await?.ok_or("missing count")?.get::<i64>(0)?, 1);
    drop(rows);
    drop(archive);
    drop(registry);
    assert!(Journal::open(&path).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn migrates_genuine_v41_journal_and_preserves_completed_intent() -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("journal.db");
    let mut decoder =
        flate2::read::GzDecoder::new(&include_bytes!("fixtures/journal-v41.db.gz")[..]);
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes)?;
    fs::write(&path, bytes)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let journal = Journal::open(&path).await?;
    assert_eq!(journal.read(1).await?, IntentState::Done);
    drop(journal);
    let conn = connection(&path).await?;
    let mut rows = conn.query("PRAGMA user_version", ()).await?;
    assert_eq!(
        rows.next().await?.ok_or("missing version")?.get::<i64>(0)?,
        42
    );
    Ok(())
}

#[tokio::test]
async fn journal_reopens_valid_registry_entries_and_rejects_corrupt_rows() -> TestResult {
    for case in ["valid", "engine", "receipt", "runtime", "conflict"] {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("journal.db");
        new_journal(&path).await?;
        let conn = connection(&path).await?;
        let digest = format!("sha256:{}", "e".repeat(64));
        let receipt =
            format!("{{\"store\":\"classic\",\"runtime_id\":\"{digest}\",\"manifest_size\":1}}");
        insert_registry(
            &conn,
            if case == "engine" {
                "bad engine"
            } else {
                "engine-a"
            },
            "prepared",
            None,
            None,
        )
        .await?;
        insert_registry(
            &conn,
            "engine-b",
            "completed",
            Some(if case == "runtime" {
                format!("sha256:{}", "f".repeat(64))
            } else {
                digest
            }),
            Some(if case == "receipt" {
                "{}".to_owned()
            } else {
                receipt
            }),
        )
        .await?;
        if case == "conflict" {
            let mut rows = conn.query("SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'image_imports_unresolved_insert'", ()).await?;
            let trigger: String = rows.next().await?.ok_or("missing trigger")?.get(0)?;
            drop(rows);
            conn.execute("DROP TRIGGER image_imports_unresolved_insert", ())
                .await?;
            conn.execute("INSERT INTO image_imports (engine_id,docker_root_digest,daemon_architecture,source_sha,archive_sha256,index_digest,platform,manifest_digest,config_digest,attempt,state,image_id) VALUES ('engine-a',?1,'x86_64',?2,?3,?4,'linux/amd64',?5,?6,0,'prepared',NULL)", ("a".repeat(64), "b".repeat(40), "c".repeat(64), "d".repeat(64), format!("sha256:{}", "d".repeat(64)), format!("sha256:{}", "e".repeat(64)))).await?;
            conn.execute(&trigger, ()).await?;
            let mut rows = conn.query("SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'image_imports_unresolved_insert'", ()).await?;
            assert_eq!(
                rows.next()
                    .await?
                    .ok_or("missing restored trigger")?
                    .get::<String>(0)?,
                trigger
            );
            drop(rows);
            let mut rows = conn.query("SELECT count(*) FROM image_imports WHERE engine_id = 'engine-a' AND state = 'prepared'", ()).await?;
            assert_eq!(
                rows.next()
                    .await?
                    .ok_or("missing conflict")?
                    .get::<i64>(0)?,
                1
            );
            drop(rows);
        }
        let mut rows = conn
            .query("SELECT count(*) FROM image_import_registry", ())
            .await?;
        assert_eq!(rows.next().await?.ok_or("missing count")?.get::<i64>(0)?, 2);
        drop(rows);
        let mut rows = conn
            .query(
                "SELECT engine_id, runtime_id, receipt FROM image_import_registry ORDER BY id",
                (),
            )
            .await?;
        let first = rows.next().await?.ok_or("missing prepared row")?;
        assert_eq!(
            first.get::<String>(0)?,
            if case == "engine" {
                "bad engine"
            } else {
                "engine-a"
            }
        );
        let second = rows.next().await?.ok_or("missing completed row")?;
        let expected_runtime = format!(
            "sha256:{}",
            (if case == "runtime" { "f" } else { "e" }).repeat(64)
        );
        assert_eq!(second.get::<String>(1)?, expected_runtime);
        let expected_receipt = if case == "receipt" {
            "{}".to_owned()
        } else {
            format!(
                "{{\"store\":\"classic\",\"runtime_id\":\"sha256:{}\",\"manifest_size\":1}}",
                "e".repeat(64)
            )
        };
        assert_eq!(second.get::<String>(2)?, expected_receipt);
        assert!(rows.next().await?.is_none());
        drop(rows);
        drop(conn);
        let reopened = Journal::open(&path).await;
        assert_eq!(reopened.is_ok(), case == "valid", "{case}");
    }
    Ok(())
}

#[tokio::test]
async fn journal_rejects_missing_registry_table_or_index() -> TestResult {
    for sql in [
        "DROP INDEX image_import_registry_engine_state",
        "DROP TABLE image_import_registry",
        "DROP TRIGGER image_imports_unresolved_insert",
    ] {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("journal.db");
        new_journal(&path).await?;
        let conn = connection(&path).await?;
        conn.execute(sql, ()).await?;
        drop(conn);
        assert!(Journal::open(&path).await.is_err(), "{sql}");
    }
    Ok(())
}

#[tokio::test]
async fn journal_rejects_modified_unresolved_import_trigger() -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("journal.db");
    new_journal(&path).await?;
    let conn = connection(&path).await?;
    conn.execute("DROP TRIGGER image_imports_unresolved_insert", ())
        .await?;
    conn.execute("CREATE TRIGGER image_imports_unresolved_insert BEFORE INSERT ON image_imports BEGIN SELECT 1; END", ()).await?;
    drop(conn);
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}
