use super::*;

#[tokio::test]
async fn version_twenty_five_migration_installs_initializer_handoff_guards() {
    let scratch = Scratch::new();
    let path = scratch.file();
    crate::journal::schema::historical_fixture::install(&path, 25)
        .await
        .expect("Could not create the version 25 schema.");
    assert_v25_migration(&path).await;
}

async fn assert_v25_migration(path: &std::path::Path) {
    drop(
        Journal::open(path)
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
        super::super::super::schema::JOURNAL_VERSION
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
