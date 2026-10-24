//! Migration paths reject corrupt stored tables without keeping partial upgrades.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::Journal;

#[path = "migration_tests/version_seven_fixture.rs"]
pub(super) mod version_seven_fixture;

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

#[path = "migration_tests/network_and_publication.rs"]
mod network_and_publication;

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
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("open historical SQLite");
    let connection = database.connect().expect("connect historical SQLite");
    version_seven_fixture::install(&connection)
        .await
        .expect("install version 7 schema");
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
    for name in [
        "execution_publication_context_insert_guard",
        "execution_publication_context_update_guard",
        "execution_publication_context_delete_guard",
        "execution_publication_parent_cleanup_guard",
        "execution_publication_parent_release_guard",
    ] {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
            .await
            .expect("remove later-version trigger");
    }
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
async fn version_seventeen_requires_complete_provisioning_cleanup_schema() {
    let required_tables = [
        "capacity_reservation_provisioning_state",
        "capacity_reservation_provisioning_cleanups",
    ];
    for missing_tables in [
        required_tables.as_slice(),
        &required_tables[..1],
        &required_tables[1..],
    ] {
        let scratch = Scratch::new();
        let path = scratch.file();
        drop(Journal::open(&path).await.expect("fresh v17 schema"));
        let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
            .build()
            .await
            .expect("open SQLite");
        let connection = database.connect().expect("connect SQLite");
        assert_eq!(
            read_version(&connection).await,
            super::schema::JOURNAL_VERSION
        );
        for table in missing_tables {
            connection
                .execute(&format!("DROP TABLE {table}"), ())
                .await
                .expect("remove provisioning table");
        }
        drop(connection);
        drop(database);

        assert!(
            Journal::open(&path).await.is_err(),
            "missing {missing_tables:?}"
        );
    }
}

#[tokio::test]
async fn version_seven_migration_rejects_malformed_source_table_atomically() {
    let scratch = Scratch::new();
    let path = scratch.file();
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("open historical SQLite");
    let connection = database.connect().expect("connect historical SQLite");
    version_seven_fixture::install(&connection)
        .await
        .expect("install version 7 schema");
    connection
        .execute(
            "CREATE TABLE connection_source_authorizations (connection_id TEXT, request_id INTEGER)",
            (),
        )
        .await
        .expect("create malformed source table");
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
