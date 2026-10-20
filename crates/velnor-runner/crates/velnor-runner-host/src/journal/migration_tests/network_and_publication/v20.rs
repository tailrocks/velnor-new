use super::*;

#[tokio::test]
async fn version_twenty_migration_adds_private_network_journal() {
    let scratch = Scratch::new();
    let path = scratch.file();
    crate::journal::schema::historical_fixture::install(&path, 20)
        .await
        .expect("Could not create the version 20 schema.");
    let database = turso::Builder::new_local(path.to_str().expect("UTF-8 temp path"))
        .build()
        .await
        .expect("open SQLite");
    let connection = database.connect().expect("connect SQLite");
    assert_eq!(read_version(&connection).await, 20);
    assert_eq!(
        table_count(&connection, "capacity_reservation_networks").await,
        0
    );
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
