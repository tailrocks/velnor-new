use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::journal::Journal;

#[path = "publication_fixture.rs"]
mod publication_fixture;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-cleanup-observer-v30-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
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
async fn version_twenty_nine_probe_constraints_migrate_without_losing_rows() {
    let scratch = Scratch::new();
    let path = scratch.file();
    drop(Journal::open(&path).await.expect("create current journal"));
    let publication_id = publication_fixture::create(&path).await;
    create_v29_fixture(&path).await;
    migrate_and_assert_v30(&path, &publication_id).await;
}

async fn create_v29_fixture(path: &std::path::Path) {
    let (database, connection) = open_database(path).await;
    let table_sql = schema_sql(&connection, "resource_probe_operations").await;
    let indexes = index_sql(&connection, "resource_probe_operations").await;
    let triggers = triggers_referencing(&connection, "resource_probe_operations").await;
    connection
        .execute(
            "DROP TRIGGER execution_publication_context_insert_guard",
            (),
        )
        .await
        .expect("drop trigger for column change");
    for (name, _) in &triggers {
        connection
            .execute(&format!("DROP TRIGGER IF EXISTS {name}"), ())
            .await
            .expect("remove probe guard for table replacement");
    }
    let old_sql = version_twenty_nine_table(&table_sql);
    connection
        .execute(&old_sql, ())
        .await
        .expect("create v29 table");
    connection
        .execute(
            "INSERT INTO resource_probe_operations_v29 SELECT * FROM resource_probe_operations",
            (),
        )
        .await
        .expect("preserve existing probe operations");
    let operation_id = "0123456789abcdef0123456789abcdef";
    insert_probe(&connection, operation_id).await;
    insert_doctor_child(&connection, operation_id).await;
    connection
        .execute("DROP TABLE resource_probe_operations", ())
        .await
        .expect("drop current probe table");
    connection
        .execute(
            "ALTER TABLE resource_probe_operations_v29 RENAME TO resource_probe_operations",
            (),
        )
        .await
        .expect("restore v29 table name");
    restore_probe_objects(&connection, indexes, triggers).await;
    connection
        .execute("PRAGMA user_version = 29", ())
        .await
        .expect("mark v29");
    assert_doctor_reference_is_intact(&connection).await;
    drop(connection);
    drop(database);
}

async fn restore_probe_objects(
    connection: &turso::Connection,
    indexes: Vec<String>,
    triggers: Vec<(String, String)>,
) {
    for index in indexes {
        connection
            .execute(&index, ())
            .await
            .expect("restore probe index");
    }
    for (name, sql) in triggers {
        if !name.starts_with("cleanup_observer_") {
            connection
                .execute(&sql, ())
                .await
                .expect("restore pre-v30 probe guard");
        }
    }
}

async fn assert_doctor_reference_is_intact(connection: &turso::Connection) {
    let invalid = scalar(
        connection,
        "SELECT COUNT(*) FROM doctor_probe_requests AS request JOIN journal_meta AS meta ON meta.singleton = 1 LEFT JOIN resource_probe_operations AS probe ON probe.operation_id = request.child_operation_id LEFT JOIN host_operation_leases AS lease ON lease.singleton = 1 WHERE request.instance_id != meta.instance_id OR meta.engine_id IS NULL OR request.engine_id != meta.engine_id OR request.active_slot != 1 OR (request.state = 'queued' AND request.child_operation_id IS NOT NULL) OR (request.state IN ('running', 'recovering', 'uncertain', 'completed', 'stale') AND (request.child_operation_id IS NULL OR probe.operation_id IS NULL OR probe.instance_id != request.instance_id OR probe.engine_id != request.engine_id OR probe.docker_root_digest != request.docker_root_digest OR probe.role != 'resource-probe')) OR (request.state IN ('completed', 'stale') AND (probe.phase NOT IN ('removed', 'aborted') OR (lease.operation_id = request.child_operation_id AND lease.state != 'released') OR (request.state = 'completed' AND probe.phase != 'removed'))) OR (request.state = 'failed' AND request.child_operation_id IS NOT NULL AND (probe.phase NOT IN ('removed', 'aborted') OR (lease.operation_id = request.child_operation_id AND lease.state != 'released'))) OR (request.state = 'uncertain' AND (probe.phase != 'quarantined' OR lease.operation_id IS NOT request.child_operation_id OR lease.state != 'uncertain')) OR (request.state = 'running' AND (probe.phase IN ('removed', 'aborted', 'quarantined') OR lease.operation_id IS NOT request.child_operation_id OR lease.state = 'released')) OR (request.state = 'recovering' AND (probe.phase IN ('removed', 'aborted') OR lease.operation_id IS NOT request.child_operation_id OR lease.state = 'released'))",
    )
    .await;
    assert_eq!(invalid, 0, "doctor child rows should be consistent");
}

async fn migrate_and_assert_v30(path: &std::path::Path, publication_id: &str) {
    drop(Journal::open(path).await.expect("upgrade v29 journal"));
    let (database, connection) = open_database(path).await;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await, 30);
    assert_eq!(
        scalar(
            &connection,
            "SELECT COUNT(*) FROM resource_probe_operations WHERE operation_id = '0123456789abcdef0123456789abcdef' AND probe_purpose = 'capacity' AND phase = 'removed'",
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &connection,
            "SELECT COUNT(*) FROM doctor_probe_requests WHERE child_operation_id = '0123456789abcdef0123456789abcdef' AND state = 'completed'",
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &connection,
            &format!("SELECT COUNT(*) FROM execution_publication_operations WHERE operation_id = '{publication_id}' AND parent_kind = 'job-facts'"),
        )
        .await,
        1
    );
    assert_no_foreign_key_errors(&connection).await;
    drop(connection);
    drop(database);
    drop(Journal::open(path).await.expect("reopen v30 journal"));
}

#[tokio::test]
async fn current_schema_rejects_a_changed_cleanup_guard_literal() {
    let scratch = Scratch::new();
    let path = scratch.file();
    drop(Journal::open(&path).await.expect("create current journal"));
    let (database, connection) = open_database(&path).await;
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'cleanup_observer_witness_insert_guard'",
            (),
        )
        .await
        .expect("read cleanup guard");
    let trigger: String = rows
        .next()
        .await
        .expect("read cleanup guard row")
        .expect("cleanup guard exists")
        .get(0)
        .expect("read cleanup guard SQL");
    connection
        .execute("DROP TRIGGER cleanup_observer_witness_insert_guard", ())
        .await
        .expect("drop cleanup guard");
    let changed = trigger.replace(
        "invalid cleanup observer witness binding",
        "changed cleanup observer witness binding",
    );
    assert_ne!(changed, trigger);
    connection
        .execute(&changed, ())
        .await
        .expect("install changed cleanup guard");
    drop(connection);
    drop(database);
    assert!(Journal::open(&path).await.is_err());
}

async fn open_database(path: &std::path::Path) -> (turso::Database, turso::Connection) {
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 test path"))
        .build()
        .await
        .expect("open test database");
    let connection = database.connect().expect("connect test database");
    (database, connection)
}

async fn schema_sql(connection: &turso::Connection, name: &str) -> String {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
        )
        .await
        .expect("read table schema");
    rows.next()
        .await
        .expect("read schema row")
        .expect("table schema exists")
        .get(0)
        .expect("read schema SQL")
}

async fn index_sql(connection: &turso::Connection, name: &str) -> Vec<String> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'index' AND tbl_name = ?1 AND sql IS NOT NULL",
            [name],
        )
        .await
        .expect("read table indexes");
    let mut indexes = Vec::new();
    while let Some(row) = rows.next().await.expect("read index row") {
        indexes.push(row.get(0).expect("read index SQL"));
    }
    indexes
}

async fn triggers_referencing(
    connection: &turso::Connection,
    table: &str,
) -> Vec<(String, String)> {
    let mut rows = connection
        .query(
            "SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' AND instr(sql, ?1) > 0",
            [table],
        )
        .await
        .expect("read related triggers");
    let mut triggers = Vec::new();
    while let Some(row) = rows.next().await.expect("read trigger row") {
        triggers.push((
            row.get(0).expect("read trigger name"),
            row.get(1).expect("read trigger SQL"),
        ));
    }
    triggers
}

fn version_twenty_nine_table(table_sql: &str) -> String {
    let mut old = table_sql
        .replace(
            "CREATE TABLE resource_probe_operations",
            "CREATE TABLE resource_probe_operations_v29",
        )
        .replace(
            "('capacity', 'owned-cgroups', 'cleanup-observer', 'none')",
            "('capacity', 'owned-cgroups', 'none')",
        );
    let context_start = old
        .find("request_context TEXT CHECK")
        .expect("current request-context constraint");
    old.truncate(context_start);
    old.push_str(
        "request_context TEXT CHECK (request_context IS NULL OR length(CAST(request_context AS BLOB)) <= 8192))",
    );
    assert!(old.contains("resource_probe_operations_v29"));
    assert!(old.contains("'owned-cgroups', 'none'"));
    assert!(old.contains("<=8192") || old.contains("<= 8192"), "{old}");
    assert!(!old.contains("'cleanup-observer'"), "{old}");
    old
}

async fn insert_probe(connection: &turso::Connection, operation_id: &str) {
    let mut rows = connection
        .query(
            "SELECT instance_id FROM journal_meta WHERE singleton = 1",
            (),
        )
        .await
        .expect("read journal instance");
    let instance_id: String = rows
        .next()
        .await
        .expect("read journal instance row")
        .expect("journal instance exists")
        .get(0)
        .expect("read journal instance id");
    let engine_id = "selected-engine";
    let root_digest = "2".repeat(64);
    let source_revision = "3".repeat(40);
    let runtime_image_id = format!("sha256:{}", "4".repeat(64));
    let image_binding_digest = "5".repeat(64);
    let operation_name = format!("velnor-resource-probe-{operation_id}");
    let projection_digest = "6".repeat(64);
    let request_sha256 = crate::journal::capacity_request_sha256();
    connection
        .execute(
            "INSERT INTO resource_probe_operations_v29 (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, container_id, helper_role, probe_purpose, request_sha256, request_context) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'resource-probe', ?9, 'removed', NULL, 'resource-probe', 'capacity', ?10, NULL)",
            (
                operation_id,
                instance_id.as_str(),
                engine_id,
                root_digest.as_str(),
                source_revision.as_str(),
                runtime_image_id.as_str(),
                image_binding_digest.as_str(),
                operation_name.as_str(),
                projection_digest.as_str(),
                request_sha256.as_str(),
            ),
        )
        .await
        .expect("insert v29 operation");
}

async fn insert_doctor_child(connection: &turso::Connection, operation_id: &str) {
    let mut rows = connection
        .query(
            "SELECT instance_id FROM journal_meta WHERE singleton = 1",
            (),
        )
        .await
        .expect("read journal instance");
    let instance_id: String = rows
        .next()
        .await
        .expect("read journal instance row")
        .expect("journal instance exists")
        .get(0)
        .expect("read journal instance id");
    connection
        .execute(
            "UPDATE journal_meta SET engine_id = 'selected-engine' WHERE singleton = 1",
            (),
        )
        .await
        .expect("bind journal engine");
    connection
        .execute(
            "INSERT INTO doctor_probe_requests (nonce, instance_id, engine_id, docker_root_digest, state, child_operation_id) VALUES (?1, ?2, 'selected-engine', ?3, 'completed', ?4)",
            (
                "a".repeat(32),
                instance_id,
                "2".repeat(64),
                operation_id,
            ),
        )
        .await
        .expect("insert linked doctor child");
}

async fn scalar(connection: &turso::Connection, sql: &str) -> i64 {
    let mut rows = connection.query(sql, ()).await.expect("run query");
    rows.next()
        .await
        .expect("read query")
        .expect("query row")
        .get(0)
        .expect("read integer")
}

async fn assert_no_foreign_key_errors(connection: &turso::Connection) {
    let mut rows = connection
        .query("PRAGMA foreign_key_check", ())
        .await
        .expect("check foreign keys");
    assert!(
        rows.next()
            .await
            .expect("read foreign key result")
            .is_none()
    );
}
