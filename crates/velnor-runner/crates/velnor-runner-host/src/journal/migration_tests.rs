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

#[tokio::test]
async fn version_twenty_migration_adds_private_network_journal() {
    let scratch = Scratch::new();
    let path = scratch.file();
    drop(Journal::open(&path).await.expect("initial schema"));
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("open SQLite");
    let connection = database.connect().expect("connect SQLite");
    connection
        .execute("DROP TRIGGER reservation_network_insert_guard", ())
        .await
        .expect("remove new network trigger");
    connection
        .execute("DROP TRIGGER reservation_network_identity_immutable", ())
        .await
        .expect("remove new network trigger");
    connection
        .execute("DROP TRIGGER reservation_network_no_delete", ())
        .await
        .expect("remove new network trigger");
    connection
        .execute("DROP TRIGGER reservation_network_transition", ())
        .await
        .expect("remove new network trigger");
    connection
        .execute("DROP TRIGGER reservation_network_start_guard", ())
        .await
        .expect("remove new network trigger");
    connection
        .execute("DROP TRIGGER reservation_network_release_guard", ())
        .await
        .expect("remove new network trigger");
    connection
        .execute("DROP TABLE capacity_reservation_networks", ())
        .await
        .expect("restore version twenty shape");
    connection
        .execute("PRAGMA user_version = 20", ())
        .await
        .expect("mark version twenty");
    drop(connection);
    drop(database);

    drop(Journal::open(&path).await.expect("migrate version twenty"));
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
        table_count(&connection, "capacity_reservation_networks").await,
        1
    );
}

#[tokio::test]
async fn version_twenty_five_migration_installs_initializer_handoff_guards() {
    let scratch = Scratch::new();
    let path = scratch.file();
    drop(Journal::open(&path).await.expect("initial schema"));
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("open SQLite");
    let connection = database.connect().expect("connect SQLite");
    for trigger in [
        "storage_handoff_insert_guard",
        "storage_handoff_transition_guard",
        "storage_handoff_no_delete",
        "storage_initializer_remove_requires_handoff",
    ] {
        connection
            .execute(&format!("DROP TRIGGER {trigger}"), ())
            .await
            .expect("remove v26 trigger");
    }
    connection
        .execute("DROP TRIGGER storage_initialization_v2_insert_guard", ())
        .await
        .expect("remove v22 metadata guard");
    connection
        .execute(
            "CREATE TRIGGER storage_initialization_v2_insert_guard BEFORE INSERT ON capacity_reservation_storage_initialization WHEN NEW.layout_version = 2 AND NEW.metadata_volume NOT GLOB 'velnor-*' BEGIN SELECT RAISE(ABORT, 'The storage initializer record is incorrect.'); END",
            (),
        )
        .await
        .expect("restore old metadata guard");
    connection
        .execute("DROP TABLE capacity_reservation_storage_handoffs", ())
        .await
        .expect("restore version twenty-five shape");
    connection
        .execute("PRAGMA user_version = 25", ())
        .await
        .expect("mark version twenty-five");
    drop(connection);
    drop(database);

    drop(
        Journal::open(&path)
            .await
            .expect("migrate version twenty-five"),
    );
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
        table_count(&connection, "capacity_reservation_storage_handoffs").await,
        1
    );
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'storage_initialization_v2_insert_guard'",
            (),
        )
        .await
        .expect("read repaired metadata guard");
    let guard = rows
        .next()
        .await
        .expect("read metadata guard row")
        .expect("metadata guard");
    let guard_sql = guard.get::<String>(0).expect("metadata guard SQL");
    let compact_guard: String = guard_sql.chars().filter(|ch| !ch.is_whitespace()).collect();
    assert!(compact_guard.contains("substr(NEW.metadata_volume,34)"));
    assert!(compact_guard.contains("'-metadata'"));
    assert!(compact_guard.contains("length(NEW.metadata_volume)!=42"));
    assert!(!guard_sql.contains("velnor-*"));
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND name IN ('storage_handoff_insert_guard', 'storage_handoff_transition_guard', 'storage_handoff_no_delete', 'storage_initializer_remove_requires_handoff')",
            (),
        )
        .await
        .expect("read handoff trigger count");
    assert_eq!(
        rows.next()
            .await
            .expect("read trigger count")
            .expect("trigger count")
            .get::<i64>(0)
            .expect("count integer"),
        4
    );
}

#[tokio::test]
async fn version_twenty_three_migration_installs_publication_guards() {
    let scratch = Scratch::new();
    let path = scratch.file();
    let journal = Journal::open(&path).await.expect("initial schema");
    journal
        .bind_engine("probe-engine")
        .await
        .expect("bind engine");
    journal
        .configure_capacity_with_operational_limits(
            super::CapacityLimits {
                jobs: 1,
                cpu_millicores: 1_000,
                memory_bytes: 4 * 1024 * 1024 * 1024,
            },
            super::OperationalCapacityLimits {
                cpu_millicores: 100,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await
        .expect("configure capacity");
    let operation_id = "a".repeat(32);
    let seed = super::ProbeSeed {
        operation_id: operation_id.clone(),
        instance_id: journal.instance_id().await.expect("instance ID"),
        engine_id: "probe-engine".to_owned(),
        docker_root_digest: "b".repeat(64),
        source_revision: "c".repeat(40),
        runtime_image_id: format!("sha256:{}", "d".repeat(64)),
        image_binding_digest: "e".repeat(64),
        operation_name: format!("velnor-resource-probe-{operation_id}"),
        projection_digest: "f".repeat(64),
        purpose: super::ResourceProbePurpose::Capacity,
        request_sha256: super::resource_probe::capacity_request_sha256(),
    };
    journal.prepare_probe(seed).await.expect("prepare probe");
    drop(journal);
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
            .expect("remove v24 trigger");
    }
    connection
        .execute("DROP TABLE execution_publication_operations", ())
        .await
        .expect("restore v23 schema");
    connection
        .execute("PRAGMA user_version = 23", ())
        .await
        .expect("mark v23 schema");
    drop(connection);
    drop(database);

    drop(Journal::open(&path).await.expect("migrate v23 schema"));
    drop(Journal::open(&path).await.expect("reopen current schema"));
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
        table_count(&connection, "execution_publication_operations").await,
        1
    );
    for name in [
        "execution_publication_context_insert_guard",
        "execution_publication_context_update_guard",
        "execution_publication_context_delete_guard",
        "execution_publication_parent_cleanup_guard",
        "execution_publication_parent_release_guard",
    ] {
        assert_eq!(table_count(&connection, name).await, 1, "{name}");
    }
    let mut rows = connection
        .query("SELECT COUNT(*) FROM execution_publication_operations", ())
        .await
        .expect("count publication rows");
    assert_eq!(
        rows.next()
            .await
            .expect("read publication count")
            .expect("publication count row")
            .get::<i64>(0)
            .expect("publication count"),
        0
    );
    let mut triggers = connection
        .query(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND name IN ('execution_publication_context_update_guard', 'execution_publication_context_delete_guard')",
            (),
        )
        .await
        .expect("read publication guards");
    assert_eq!(
        triggers
            .next()
            .await
            .expect("read guard count")
            .expect("guard count row")
            .get::<i64>(0)
            .expect("guard count"),
        2
    );
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'execution_publication_context_insert_guard'",
            (),
        )
        .await
        .expect("read insert guard SQL");
    let insert_guard_sql = rows
        .next()
        .await
        .expect("read insert guard")
        .expect("insert guard exists")
        .get::<String>(0)
        .expect("insert guard SQL");
    connection
        .execute("PRAGMA foreign_keys = OFF", ())
        .await
        .expect("disable foreign keys for guard fixture");
    connection
        .execute(
            "DROP TRIGGER execution_publication_context_insert_guard",
            (),
        )
        .await
        .expect("allow one guard fixture row");
    let digest = "f".repeat(64);
    connection
        .execute(
            "INSERT INTO execution_publication_operations (operation_id, parent_kind, parent_operation_id, reservation_id, connection_id, scale_set_id, witness_request_id, actual_request_id, runner_id, runner_name, runner_container_id, dind_container_id, engine_id, docker_root_digest, metadata_volume_name, metadata_volume_id, metadata_layout_version, metadata_layout_digest, guest_boot_id, payload_sha256, claim_digest, projection_digest, context_digest) VALUES (?1, 'challenge', 1, 1, 'test-connection', 1, 1, NULL, 1, 'test-runner', 'runner-container-1', 'dind-container-1', 'probe-engine', ?2, 'metadata-volume', 'metadata-volume-id', 2, ?2, '11111111-2222-4333-8444-555555555555', ?2, ?2, ?2, ?2)",
            (operation_id.as_str(), digest.as_str()),
        )
        .await
        .expect("insert one guard fixture row");
    connection
        .execute(&insert_guard_sql, ())
        .await
        .expect("restore insert guard");
    assert!(connection
        .execute(
            "UPDATE execution_publication_operations SET payload_sha256 = ?1 WHERE operation_id = ?2",
            ("e".repeat(64), operation_id.as_str()),
        )
        .await
        .is_err());
    assert!(
        connection
            .execute(
                "DELETE FROM execution_publication_operations WHERE operation_id = ?1",
                [operation_id.as_str()],
            )
            .await
            .is_err()
    );
    drop(connection);
    drop(database);
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("reopen migrated database");
    let connection = database.connect().expect("connect migrated database");
    assert_eq!(
        read_version(&connection).await,
        super::schema::JOURNAL_VERSION
    );
    let mut rows = connection
        .query(
            "SELECT phase, helper_role, probe_purpose, request_sha256 FROM resource_probe_operations WHERE operation_id = ?1",
            [operation_id.as_str()],
        )
        .await
        .expect("read original probe row");
    let row = rows
        .next()
        .await
        .expect("read probe row")
        .expect("probe remains");
    assert_eq!(row.get::<String>(0).expect("probe phase"), "prepared");
    assert_eq!(row.get::<String>(1).expect("helper role"), "resource-probe");
    assert_eq!(row.get::<String>(2).expect("probe purpose"), "capacity");
    assert_eq!(
        row.get::<String>(3).expect("request digest"),
        super::resource_probe::capacity_request_sha256()
    );
    assert!(rows.next().await.expect("unique probe row").is_none());
    drop(connection);
    drop(database);
    assert!(Journal::open(&path).await.is_err());
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
