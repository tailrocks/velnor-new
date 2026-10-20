use super::*;

#[tokio::test]
async fn version_twenty_three_migration_installs_publication_guards() {
    let scratch = Scratch::new();
    let path = scratch.file();
    let operation_id = prepare_v23_probe(&path).await;
    restore_v23_schema(&path).await;
    assert_v23_migration(&path, &operation_id).await;
}

async fn prepare_v23_probe(path: &std::path::Path) -> String {
    crate::journal::schema::historical_fixture::install(path, 23)
        .await
        .expect("Could not create the version 23 schema.");
    let database = turso::Builder::new_local(
        path.to_str()
            .expect("The temporary database path must use UTF-8."),
    )
    .build()
    .await
    .expect("Could not open the version 23 database.");
    let connection = database
        .connect()
        .expect("Could not connect to the version 23 database.");
    let operation_id = "a".repeat(32);
    let mut rows = connection
        .query(
            "SELECT instance_id FROM journal_meta WHERE singleton = 1",
            (),
        )
        .await
        .expect("Could not read the journal instance ID.");
    let instance_id: String = rows
        .next()
        .await
        .expect("Could not read a row from the journal instance table.")
        .expect("The journal instance table has no row.")
        .get(0)
        .expect("The journal instance row has no instance ID.");
    connection
        .execute(
            "UPDATE journal_meta SET engine_id = 'probe-engine' WHERE singleton = 1",
            (),
        )
        .await
        .expect("Could not set the probe engine.");
    connection
        .execute(
            "UPDATE host_capacity_policy SET operational_cpu_millicores = 100, operational_memory_bytes = 134217728 WHERE singleton = 1",
            (),
        )
        .await
        .expect("Could not set the probe capacity.");
    connection
        .execute(
            "INSERT INTO host_operation_leases (singleton, operation_id, engine_id, docker_root_digest, operation_name, container_id, cpu_millicores, memory_bytes, state) VALUES (1, ?1, 'probe-engine', ?2, ?3, NULL, 100, 134217728, 'reserved')",
            (
                operation_id.as_str(),
                "b".repeat(64),
                format!("velnor-resource-probe-{operation_id}"),
            ),
        )
        .await
        .expect("Could not save the probe lease.");
    connection
        .execute(
            "INSERT INTO resource_probe_operations (operation_id, instance_id, engine_id, docker_root_digest, source_revision, runtime_image_id, image_binding_digest, operation_name, role, projection_digest, phase, container_id) VALUES (?1, ?2, 'probe-engine', ?3, ?4, ?5, ?6, ?7, 'resource-probe', ?8, 'prepared', NULL)",
            (
                operation_id.as_str(),
                instance_id,
                "b".repeat(64),
                "c".repeat(40),
                format!("sha256:{}", "d".repeat(64)),
                "e".repeat(64),
                format!("velnor-resource-probe-{operation_id}"),
                "f".repeat(64),
            ),
        )
        .await
        .expect("Could not record the version 23 probe.");
    drop(connection);
    drop(database);
    operation_id
}

async fn restore_v23_schema(path: &std::path::Path) {
    let database = turso::Builder::new_local(
        path.to_str()
            .expect("The temporary database path must use UTF-8."),
    )
    .build()
    .await
    .expect("open SQLite");
    let connection = database.connect().expect("connect SQLite");
    assert_eq!(read_version(&connection).await, 23);
    drop(connection);
    drop(database);
}

async fn assert_v23_migration(path: &std::path::Path, operation_id: &str) {
    drop(Journal::open(&path).await.expect("migrate v23 schema"));
    drop(Journal::open(&path).await.expect("reopen current schema"));
    let database = turso::Builder::new_local(
        path.to_str()
            .expect("The temporary database path must use UTF-8."),
    )
    .build()
    .await
    .expect("reopen SQLite");
    let connection = database.connect().expect("connect SQLite");
    let insert_guard_sql = assert_v23_publication_schema(&connection).await;
    assert_v23_guard_rejections(&connection, operation_id, &insert_guard_sql).await;
    drop(connection);
    drop(database);
    assert_v23_probe_survives_reopen(path, operation_id).await;
}

async fn assert_v23_publication_schema(connection: &turso::Connection) -> String {
    assert_eq!(
        read_version(connection).await,
        super::super::super::schema::JOURNAL_VERSION
    );
    assert_eq!(
        table_count(connection, "execution_publication_operations").await,
        1
    );
    for name in [
        "execution_publication_context_insert_guard",
        "execution_publication_context_update_guard",
        "execution_publication_context_delete_guard",
        "execution_publication_parent_cleanup_guard",
        "execution_publication_parent_release_guard",
    ] {
        assert_eq!(table_count(connection, name).await, 1, "{name}");
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
    insert_guard_sql
}

async fn assert_v23_guard_rejections(
    connection: &turso::Connection,
    operation_id: &str,
    insert_guard_sql: &str,
) {
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
            (operation_id, digest.as_str()),
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
            ("e".repeat(64), operation_id),
        )
        .await
        .is_err());
    assert!(
        connection
            .execute(
                "DELETE FROM execution_publication_operations WHERE operation_id = ?1",
                [operation_id],
            )
            .await
            .is_err()
    );
}

async fn assert_v23_probe_survives_reopen(path: &std::path::Path, operation_id: &str) {
    let database = turso::Builder::new_local(
        path.to_str()
            .expect("The temporary database path must use UTF-8."),
    )
    .build()
    .await
    .expect("reopen migrated database");
    let connection = database.connect().expect("connect migrated database");
    assert_eq!(
        read_version(&connection).await,
        super::super::super::schema::JOURNAL_VERSION
    );
    let mut rows = connection
        .query(
            "SELECT phase, helper_role, probe_purpose, request_sha256 FROM resource_probe_operations WHERE operation_id = ?1",
            [operation_id],
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
        super::super::super::resource_probe::capacity_request_sha256()
    );
    assert!(rows.next().await.expect("unique probe row").is_none());
    drop(connection);
    drop(database);
    assert!(Journal::open(&path).await.is_err());
}
