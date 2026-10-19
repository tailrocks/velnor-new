use super::*;

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
        super::super::super::schema::JOURNAL_VERSION
    );
    assert_eq!(
        table_count(&connection, "capacity_reservation_networks").await,
        1
    );
}
