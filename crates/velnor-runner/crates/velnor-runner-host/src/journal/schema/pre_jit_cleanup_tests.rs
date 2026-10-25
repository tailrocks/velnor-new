#[path = "pre_jit_cleanup_tests/fixture_stack_cases.rs"]
mod fixture_stack_cases;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::journal::Journal;

struct Scratch(PathBuf);

#[path = "pre_jit_cleanup_tests/missing_create_spec_table.rs"]
mod missing_create_spec_table;
#[path = "pre_jit_cleanup_tests/settlement_schema.rs"]
mod settlement_schema;
#[path = "pre_jit_cleanup_tests/version_downgrade.rs"]
mod version_downgrade;

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "velnor-pre-jit-v32-{}-{serial}",
            std::process::id()
        )))
    }

    fn database(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

async fn rejects_missing_v32_trigger(trigger: &str) -> Result<(), String> {
    let scratch = Scratch::new();
    std::fs::create_dir_all(&scratch.0).map_err(|_| "The test directory could not be created.")?;
    let path = scratch.database();
    drop(
        Journal::open(&path)
            .await
            .map_err(|_| "The journal could not be created.")?,
    );
    let path_text = path.to_str().ok_or("The journal path is not UTF-8.")?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| "The test database could not be opened.")?;
    let connection = database
        .connect()
        .map_err(|_| "The test connection could not be opened.")?;
    connection
        .execute(&format!("DROP TRIGGER {trigger}"), ())
        .await
        .map_err(|_| "The test trigger could not be removed.")?;
    drop(connection);
    drop(database);
    if Journal::open(&path).await.is_ok() {
        return Err("The current-schema check repaired a missing v32 trigger.".to_owned());
    }
    Ok(())
}

async fn create_v31_database(path: &std::path::Path) -> Result<(), String> {
    let path_text = path.to_str().ok_or("The journal path is not UTF-8.")?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| "The test database could not be opened.")?;
    let connection = database
        .connect()
        .map_err(|_| "The test connection could not be opened.")?;
    connection
        .execute_batch(include_str!("pre_jit_cleanup_tests/v31_empty.sql"))
        .await
        .map_err(|error| format!("The version 31 fixture could not be loaded: {error}"))?;
    let version = super::super::journal_version(&connection)
        .await
        .map_err(|_| "The fixture version could not be read.")?;
    if version != 31 {
        return Err("The fixture must use journal version 31.".to_owned());
    }
    drop(connection);
    drop(database);
    Ok(())
}

#[tokio::test]
async fn version_thirty_one_empty_fixture_upgrade_installs_only_the_typed_create_receipt()
-> Result<(), String> {
    let scratch = Scratch::new();
    std::fs::create_dir_all(&scratch.0).map_err(|_| "The test directory could not be created.")?;
    let path = scratch.database();
    create_v31_database(&path).await?;
    let path_text = path.to_str().ok_or("The journal path is not UTF-8.")?;

    drop(
        Journal::open(&path)
            .await
            .map_err(|_| "The journal could not be upgraded.")?,
    );
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| "The test database could not be reopened.")?;
    let connection = database
        .connect()
        .map_err(|_| "The test connection could not be reopened.")?;
    super::validate(&connection)
        .await
        .map_err(|_| "The upgraded schema did not validate.")?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_pre_jit_create_responses",
            (),
        )
        .await
        .map_err(|_| "The create receipt count could not be read.")?;
    let count: i64 = rows
        .next()
        .await
        .map_err(|_| "The create receipt count could not be read.")?
        .ok_or("The create receipt count is missing.")?
        .get(0)
        .map_err(|_| "The create receipt count is invalid.")?;
    if count != 0 {
        return Err("The upgrade must not invent create receipts.".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn create_receipt_rejects_unsupported_response_status() -> Result<(), String> {
    let scratch = Scratch::new();
    std::fs::create_dir_all(&scratch.0).map_err(|_| "The test directory could not be created.")?;
    let path = scratch.database();
    drop(
        Journal::open(&path)
            .await
            .map_err(|_| "The journal could not be created.")?,
    );
    let path_text = path
        .to_str()
        .ok_or("The journal path is not UTF-8.")?
        .to_owned();
    let database = turso::Builder::new_local(&path_text)
        .build()
        .await
        .map_err(|_| "The test database could not be opened.")?;
    let connection = database
        .connect()
        .map_err(|_| "The test connection could not be opened.")?;
    connection
        .execute("PRAGMA foreign_keys = OFF", ())
        .await
        .map_err(|_| "The test connection could not be configured.")?;
    connection
        .execute("DROP TRIGGER pre_jit_create_response_insert_guard", ())
        .await
        .map_err(|_| "The receipt insert guard could not be removed.")?;
    connection
        .execute(
            "INSERT INTO capacity_reservation_pre_jit_create_responses (reservation_id, role, connection_id, operation_id, engine_id, docker_root_digest, container_name, client_api_major, client_api_minor, response_status) VALUES (1, 'runner', 'connection', 1, 'engine', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'runner', 1, 53, 409)",
            (),
        )
        .await
        .map_err(|_| "The schema rejected supported receipt fields.")?;
    connection
        .execute("DROP TRIGGER pre_jit_create_response_no_delete", ())
        .await
        .map_err(|_| "The receipt retention guard could not be removed.")?;
    connection
        .execute(
            "DELETE FROM capacity_reservation_pre_jit_create_responses WHERE reservation_id = 1",
            (),
        )
        .await
        .map_err(|_| "The supported receipt could not be cleared.")?;
    let rejected = connection
        .execute(
            "INSERT INTO capacity_reservation_pre_jit_create_responses (reservation_id, role, connection_id, operation_id, engine_id, docker_root_digest, container_name, client_api_major, client_api_minor, response_status) VALUES (1, 'runner', 'connection', 1, 'engine', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'runner', 1, 53, 408)",
            (),
        )
        .await;
    if rejected.is_ok() {
        return Err("The schema accepted an unsupported daemon status.".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn current_schema_rejects_missing_pre_jit_progress_guard() -> Result<(), String> {
    rejects_missing_v32_trigger("pre_jit_cleanup_progress_update_guard").await
}

#[tokio::test]
async fn current_schema_rejects_missing_terminal_create_guard() -> Result<(), String> {
    rejects_missing_v32_trigger("pre_jit_create_response_insert_guard").await
}

#[tokio::test]
async fn current_schema_rejects_missing_create_spec_guard() -> Result<(), String> {
    rejects_missing_v32_trigger("pre_jit_create_spec_insert_guard").await
}

#[test]
fn runner_create_terminal_reply_allows_one_live_pre_jit_claim() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "runner_create_terminal_reply_allows_one_live_pre_jit_claim",
        || {
            Box::pin(fixture_stack_cases::runner_create_terminal_reply_allows_one_live_pre_jit_claim_on_fixture_stack())
        },
    )
}

#[test]
fn claimed_pre_jit_recovery_accepts_progress_without_changing_reservation_identity()
-> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "claimed_pre_jit_recovery_accepts_progress_without_changing_reservation_identity",
        || {
            Box::pin(fixture_stack_cases::claimed_pre_jit_recovery_accepts_progress_without_changing_reservation_identity_on_fixture_stack())
        },
    )
}
