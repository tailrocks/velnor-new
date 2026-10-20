use super::*;

#[tokio::test]
async fn probe_constraint_upgrade_preserves_published_parent_row() {
    let scratch = Scratch::new();
    let path = scratch.file();
    drop(Journal::open(&path).await.expect("create current journal"));
    let publication_id = publication_fixture::create(&path).await;
    let (indexes, triggers) = downgrade_probe_table_for_column_upgrade(&path).await;
    let (database, connection) = open_database(&path).await;
    crate::journal::schema::execution_publication::upgrade_v30(&connection)
        .await
        .expect("upgrade the probe columns");
    restore_probe_objects(&connection, indexes, triggers).await;
    assert_doctor_reference_is_intact(&connection).await;
    drop(connection);
    drop(database);
    migrate_and_assert_v30(&path, Some(&publication_id)).await;
}

async fn downgrade_probe_table_for_column_upgrade(
    path: &std::path::Path,
) -> (Vec<String>, Vec<(String, String)>) {
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
    insert_probe(&connection, "resource_probe_operations_v29", operation_id).await;
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
    assert_doctor_reference_is_intact(&connection).await;
    drop(connection);
    drop(database);
    (indexes, triggers)
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
        if name != "execution_publication_context_insert_guard" {
            connection
                .execute(&sql, ())
                .await
                .expect("restore pre-v30 probe guard");
        }
    }
}
