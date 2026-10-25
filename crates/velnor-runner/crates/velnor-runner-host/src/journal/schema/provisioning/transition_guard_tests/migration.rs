use crate::journal::Journal;

use super::support::{Scratch, open_database, scalar};
use crate::journal::schema::provisioning::{
    LEGACY_CONTAINER_TRANSITION, LEGACY_STATE_TRANSITION, TRIGGERS, is_transition_guard,
};

#[tokio::test]
async fn version_twenty_eight_migrates_exact_old_guards_and_reopens() {
    let scratch = Scratch::new();
    let path = scratch.file();
    crate::journal::schema::historical_fixture::install(&path, 28)
        .await
        .expect("Could not install the version 28 journal fixture.");
    let (database, connection) = open_database(&path).await;
    remove_current_guards(&connection).await;
    install_legacy_guards(&connection).await;
    drop(connection);
    drop(database);

    drop(
        Journal::open(&path)
            .await
            .expect("Could not upgrade the version 28 journal."),
    );
    drop(
        Journal::open(&path)
            .await
            .expect("Could not reopen the version 30 journal."),
    );
    assert_current_guard_counts(&path).await;
}

#[tokio::test]
async fn version_twenty_eight_rejects_partial_guard_set_without_advancing() {
    let scratch = Scratch::new();
    let path = scratch.file();
    crate::journal::schema::historical_fixture::install(&path, 28)
        .await
        .expect("Could not install the version 28 journal fixture.");
    let (database, connection) = open_database(&path).await;
    connection
        .execute("DROP TRIGGER provisioning_container_created_transition", ())
        .await
        .expect("Could not remove one transition guard.");
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    let (_database, connection) = open_database(&path).await;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await, 28);
    assert_eq!(scalar(&connection, "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name GLOB 'provisioning_container_*'").await, 7);
}

#[tokio::test]
async fn version_twenty_eight_rejects_unknown_guard_sql() {
    let scratch = Scratch::new();
    let path = scratch.file();
    crate::journal::schema::historical_fixture::install(&path, 28)
        .await
        .expect("Could not install the version 28 journal fixture.");
    let (database, connection) = open_database(&path).await;
    remove_current_guards(&connection).await;
    install_legacy_guards(&connection).await;
    let changed =
        LEGACY_CONTAINER_TRANSITION.replace("OLD.role != NEW.role", "OLD.role IS NOT NEW.role");
    connection
        .execute("DROP TRIGGER provisioning_container_transition", ())
        .await
        .expect("Could not remove the known old container guard.");
    connection
        .execute(&changed, ())
        .await
        .expect("Could not add the altered old guard.");
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    let (_database, connection) = open_database(&path).await;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await, 28);
    assert_eq!(scalar(&connection, "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name IN ('provisioning_container_transition', 'provisioning_state_transition')").await, 2);
}

#[tokio::test]
async fn version_twenty_eight_rejects_mixed_guard_generations() {
    let scratch = Scratch::new();
    let path = scratch.file();
    crate::journal::schema::historical_fixture::install(&path, 28)
        .await
        .expect("Could not install the version 28 journal fixture.");
    let (database, connection) = open_database(&path).await;
    connection
        .execute(LEGACY_CONTAINER_TRANSITION, ())
        .await
        .expect("Could not add the legacy container guard.");
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    let (_database, connection) = open_database(&path).await;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await, 28);
    assert_eq!(scalar(&connection, "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name='provisioning_container_transition'").await, 1);
}

#[tokio::test]
async fn version_twenty_eight_rolls_back_when_owned_trigger_name_is_taken() {
    let scratch = Scratch::new();
    let path = scratch.file();
    crate::journal::schema::historical_fixture::install(&path, 28)
        .await
        .expect("Could not install the version 28 journal fixture.");
    let (database, connection) = open_database(&path).await;
    remove_current_guards(&connection).await;
    install_legacy_guards(&connection).await;
    connection
        .execute("CREATE TABLE unrelated_trigger_target (value INTEGER)", ())
        .await
        .expect("Could not add the conflict target table.");
    connection.execute("CREATE TRIGGER provisioning_container_created_transition BEFORE UPDATE ON unrelated_trigger_target BEGIN SELECT RAISE(ABORT, 'unrelated trigger'); END", ()).await.expect("Could not add the conflicting unrelated trigger.");
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    let (_database, connection) = open_database(&path).await;
    assert_eq!(scalar(&connection, "PRAGMA user_version").await, 28);
    assert_eq!(scalar(&connection, "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name IN ('provisioning_container_transition', 'provisioning_state_transition')").await, 2);
    assert_eq!(scalar(&connection, "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name='provisioning_container_created_transition' AND tbl_name='unrelated_trigger_target'").await, 1);
}

#[tokio::test]
async fn current_schema_rejects_extra_legacy_transition_guards() {
    let scratch = Scratch::new();
    let path = scratch.file();
    drop(
        Journal::open(&path)
            .await
            .expect("Could not create the current journal fixture."),
    );
    let (database, connection) = open_database(&path).await;
    connection
        .execute(LEGACY_CONTAINER_TRANSITION, ())
        .await
        .expect("Could not add the stale legacy guard.");
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
}

async fn remove_current_guards(connection: &turso::Connection) {
    for (name, _) in TRIGGERS
        .iter()
        .filter(|(name, _)| is_transition_guard(name))
    {
        connection
            .execute(&format!("DROP TRIGGER {name}"), ())
            .await
            .expect("Could not remove the current transition guard.");
    }
}

async fn install_legacy_guards(connection: &turso::Connection) {
    connection
        .execute(LEGACY_CONTAINER_TRANSITION, ())
        .await
        .expect("Could not add the version 28 container guard.");
    connection
        .execute(LEGACY_STATE_TRANSITION, ())
        .await
        .expect("Could not add the version 28 state guard.");
}

async fn assert_current_guard_counts(path: &std::path::Path) {
    let (_database, connection) = open_database(path).await;
    assert_eq!(
        scalar(&connection, "PRAGMA user_version").await,
        super::super::super::JOURNAL_VERSION
    );
    assert_eq!(scalar(&connection, "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name IN ('provisioning_container_transition', 'provisioning_state_transition')").await, 0);
    assert_eq!(scalar(&connection, "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name GLOB 'provisioning_container_*'").await, 8);
    assert_eq!(scalar(&connection, "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name GLOB 'provisioning_state_*'").await, 8);
}
