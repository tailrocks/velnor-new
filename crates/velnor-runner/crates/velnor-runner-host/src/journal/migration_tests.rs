//! Migration paths reject corrupted persisted tables without keeping partial upgrades.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::Journal;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-migration-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&path).expect("scratch directory");
        Self(path)
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn version_four_migration_validates_connection_and_remote_tables() {
    for damage in [
        "DROP INDEX connection_intent_live",
        "DROP TABLE connection_completion_inbox",
    ] {
        let scratch = Scratch::new();
        let path = scratch.file();
        drop(Journal::open(&path).await.expect("initial schema"));
        let path_text = path.to_str().expect("UTF-8 temp path");
        let database = turso::Builder::new_local(path_text)
            .build()
            .await
            .expect("open SQLite");
        let connection = database.connect().expect("connect SQLite");
        for table in [
            "connection_runner_jits",
            "connection_request_authorizations",
            "connection_source_authorizations",
            "connection_scale_set_intents",
        ] {
            connection
                .execute(&format!("DROP TABLE {table}"), ())
                .await
                .expect("restore version-four shape");
        }
        connection
            .execute("DROP TABLE image_imports", ())
            .await
            .expect("restore version-four shape");
        connection.execute(damage, ()).await.expect("damage schema");
        connection
            .execute("PRAGMA user_version = 4", ())
            .await
            .expect("mark version four");
        drop(connection);
        drop(database);

        assert!(Journal::open(&path).await.is_err(), "{damage}");
        assert_v4_failure_rolled_back(&path, damage).await;
    }
}

#[tokio::test]
async fn version_seven_migration_installs_current_source_authorization_schema() {
    let scratch = Scratch::new();
    let path = scratch.file();
    drop(Journal::open(&path).await.expect("initial schema"));
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("open SQLite");
    let connection = database.connect().expect("connect SQLite");
    connection
        .execute("DROP TABLE connection_source_authorizations", ())
        .await
        .expect("restore version-seven shape");
    connection
        .execute("PRAGMA user_version = 7", ())
        .await
        .expect("mark version seven");
    drop(connection);
    drop(database);

    drop(Journal::open(&path).await.expect("migrate version seven"));
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("reopen SQLite");
    let connection = database.connect().expect("connect SQLite");
    assert_eq!(
        read_version(&connection).await,
        super::schema::JOURNAL_VERSION
    );
    assert_eq!(
        table_count(&connection, "connection_source_authorizations").await,
        1
    );
}

#[tokio::test]
async fn version_eight_migration_rejects_a_broken_source_index_atomically() {
    let scratch = Scratch::new();
    let path = scratch.file();
    drop(Journal::open(&path).await.expect("initial schema"));
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("open SQLite");
    let connection = database.connect().expect("connect SQLite");
    connection
        .execute(
            "ALTER TABLE connection_source_authorizations DROP COLUMN receipt_binding_digest",
            (),
        )
        .await
        .expect("restore version-eight table shape");
    connection
        .execute("DROP INDEX connection_source_authorization_intent", ())
        .await
        .expect("drop source index");
    connection
        .execute("PRAGMA user_version = 8", ())
        .await
        .expect("mark version eight");
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("reopen SQLite");
    let connection = database.connect().expect("connect SQLite");
    assert_eq!(read_version(&connection).await, 8);
    assert_eq!(
        table_count(&connection, "connection_source_authorization_intent").await,
        0
    );
}

#[tokio::test]
async fn version_seven_migration_rejects_malformed_source_table_atomically() {
    let scratch = Scratch::new();
    let path = scratch.file();
    drop(Journal::open(&path).await.expect("initial schema"));
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("open SQLite");
    let connection = database.connect().expect("connect SQLite");
    connection
        .execute("DROP INDEX connection_source_authorization_intent", ())
        .await
        .expect("drop source index");
    connection
        .execute("DROP TABLE connection_source_authorizations", ())
        .await
        .expect("drop source table");
    connection
        .execute(
            "CREATE TABLE connection_source_authorizations (connection_id TEXT, request_id INTEGER)",
            (),
        )
        .await
        .expect("create malformed source table");
    connection
        .execute("PRAGMA user_version = 7", ())
        .await
        .expect("mark version seven");
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("reopen SQLite");
    let connection = database.connect().expect("connect SQLite");
    assert_eq!(read_version(&connection).await, 7);
    assert_eq!(
        table_count(&connection, "connection_source_authorizations").await,
        1
    );
    assert_eq!(
        table_count(&connection, "connection_source_authorization_intent").await,
        0
    );
}

async fn read_version(connection: &turso::Connection) -> i64 {
    let mut rows = connection
        .query("PRAGMA user_version", ())
        .await
        .expect("read version");
    rows.next()
        .await
        .expect("read version row")
        .expect("version row")
        .get(0)
        .expect("version value")
}

async fn table_count(connection: &turso::Connection, name: &str) -> i64 {
    let mut rows = connection
        .query("SELECT COUNT(*) FROM sqlite_schema WHERE name = ?1", [name])
        .await
        .expect("count schema objects");
    rows.next()
        .await
        .expect("read schema count")
        .expect("schema count row")
        .get(0)
        .expect("schema count")
}

async fn assert_v4_failure_rolled_back(path: &std::path::Path, damage: &str) {
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("reopen SQLite");
    let connection = database.connect().expect("connect SQLite");
    let mut rows = connection
        .query("PRAGMA user_version", ())
        .await
        .expect("read schema version");
    assert_eq!(
        rows.next()
            .await
            .expect("read version row")
            .expect("version row")
            .get::<i64>(0)
            .expect("version integer"),
        4
    );
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'image_imports'",
            (),
        )
        .await
        .expect("check partial table");
    assert_eq!(
        rows.next()
            .await
            .expect("read table count")
            .expect("table count")
            .get::<i64>(0)
            .expect("count integer"),
        0
    );
    let object = if damage.contains("INDEX") {
        "connection_intent_live"
    } else {
        "connection_completion_inbox"
    };
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = ?1",
            [object],
        )
        .await
        .expect("check original damage");
    assert_eq!(
        rows.next()
            .await
            .expect("read object count")
            .expect("object count")
            .get::<i64>(0)
            .expect("count integer"),
        0,
        "{damage}"
    );
}
