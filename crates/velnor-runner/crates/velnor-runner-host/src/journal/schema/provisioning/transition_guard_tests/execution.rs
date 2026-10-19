use super::support::{Scratch, open_database};
use crate::journal::schema::provisioning::{TABLES, reconcile_transition_guards};

#[tokio::test]
async fn split_provisioning_guards_execute_valid_transitions_on_default_stack() {
    let scratch = Scratch::new();
    let (_database, connection) = open_database(&scratch.file()).await;
    install_transition_tables(&connection).await;
    insert_container(&connection).await;
    advance_container(&connection).await;
    reject_container_identity_change(&connection).await;
    insert_state(&connection).await;
    advance_state(&connection).await;
    reject_invalid_state_transition(&connection).await;
}

async fn install_transition_tables(connection: &turso::Connection) {
    connection
        .execute("PRAGMA foreign_keys = OFF", ())
        .await
        .expect("isolate valid transition rows");
    for table_name in [
        "capacity_reservation_bootstrap_containers",
        "capacity_reservation_provisioning_state",
    ] {
        let (_, table_sql) = TABLES
            .iter()
            .find(|(name, _)| *name == table_name)
            .expect("transition table DDL");
        connection
            .execute(*table_sql, ())
            .await
            .expect("create exact transition table");
    }
    reconcile_transition_guards(connection, true)
        .await
        .expect("install exact transition guards");
}

async fn insert_container(connection: &turso::Connection) {
    connection.execute(
        "INSERT INTO capacity_reservation_bootstrap_containers (reservation_id, role, container_name, create_state, start_state) VALUES (1, 'runner', 'runner-test', 'unclaimed', 'not_requested')",
        (),
    ).await.expect("insert unclaimed runner row");
}

async fn advance_container(connection: &turso::Connection) {
    connection.execute("UPDATE capacity_reservation_bootstrap_containers SET create_state='requested' WHERE reservation_id=1 AND role='runner'", ()).await.expect("request container create");
    connection.execute("UPDATE capacity_reservation_bootstrap_containers SET create_state='created', container_id='runner-id' WHERE reservation_id=1 AND role='runner'", ()).await.expect("record created container");
    connection.execute("UPDATE capacity_reservation_bootstrap_containers SET start_state='requested' WHERE reservation_id=1 AND role='runner'", ()).await.expect("request container start");
    connection.execute("UPDATE capacity_reservation_bootstrap_containers SET start_state='started' WHERE reservation_id=1 AND role='runner'", ()).await.expect("record started container");
}

async fn reject_container_identity_change(connection: &turso::Connection) {
    assert!(connection.execute(
        "UPDATE capacity_reservation_bootstrap_containers SET container_name='changed' WHERE reservation_id=1 AND role='runner'",
        (),
    ).await.is_err());
}

async fn insert_state(connection: &turso::Connection) {
    connection.execute(
        "INSERT INTO capacity_reservation_provisioning_state (reservation_id, bootstrap_start_state) VALUES (1, 'not_requested')",
        (),
    ).await.expect("insert provisioning state");
}

async fn advance_state(connection: &turso::Connection) {
    advance_bootstrap(connection).await;
    bind_resources(connection).await;
    bind_jit(connection).await;
    publish_projection(connection).await;
    deliver_projection(connection).await;
    record_cleanup(connection).await;
}

async fn advance_bootstrap(connection: &turso::Connection) {
    connection.execute("UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state='requested' WHERE reservation_id=1", ()).await.expect("request bootstrap start");
    connection.execute("UPDATE capacity_reservation_provisioning_state SET bootstrap_start_state='started', runner_container_id='runner-id', dind_container_id='dind-id' WHERE reservation_id=1", ()).await.expect("record bootstrap start");
}

async fn bind_resources(connection: &turso::Connection) {
    connection.execute("UPDATE capacity_reservation_provisioning_state SET resources_bound=1, instance_binding_digest=?1, guest_boot_id='boot-1' WHERE reservation_id=1", ("a".repeat(64),)).await.expect("bind resources");
}

async fn bind_jit(connection: &turso::Connection) {
    connection.execute("UPDATE capacity_reservation_provisioning_state SET jit_state='requested' WHERE reservation_id=1", ()).await.expect("request JIT");
    connection.execute("UPDATE capacity_reservation_provisioning_state SET jit_state='bound', runner_id=1, runner_name='runner-test', runner_scale_set_id=1 WHERE reservation_id=1", ()).await.expect("bind JIT");
}

async fn publish_projection(connection: &turso::Connection) {
    connection.execute("UPDATE capacity_reservation_provisioning_state SET publication_state='requested' WHERE reservation_id=1", ()).await.expect("request publication");
    connection.execute("UPDATE capacity_reservation_provisioning_state SET publication_state='published', projection_sha256=?1, publication_directory_device=1, publication_directory_inode=1, publication_mount_id=1 WHERE reservation_id=1", ("b".repeat(64),)).await.expect("publish projection");
}

async fn deliver_projection(connection: &turso::Connection) {
    connection.execute("UPDATE capacity_reservation_provisioning_state SET delivery_state='requested' WHERE reservation_id=1", ()).await.expect("request delivery");
    connection.execute("UPDATE capacity_reservation_provisioning_state SET delivery_state='delivered' WHERE reservation_id=1", ()).await.expect("record delivery");
}

async fn record_cleanup(connection: &turso::Connection) {
    connection.execute("UPDATE capacity_reservation_provisioning_state SET storage_cleanup_proven=1 WHERE reservation_id=1", ()).await.expect("record storage cleanup");
}

async fn reject_invalid_state_transition(connection: &turso::Connection) {
    assert!(connection.execute(
        "UPDATE capacity_reservation_provisioning_state SET publication_state='uncertain' WHERE reservation_id=1",
        (),
    ).await.is_err());
}
