use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::journal::Journal;

struct Scratch(PathBuf);

#[path = "pre_jit_cleanup_tests/missing_create_spec_table.rs"]
mod missing_create_spec_table;
#[path = "pre_jit_cleanup_tests/settlement_schema.rs"]
mod settlement_schema;

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
async fn version_thirty_one_upgrade_installs_only_the_typed_create_receipt() -> Result<(), String> {
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

#[tokio::test]
async fn runner_create_terminal_reply_allows_one_live_pre_jit_claim() -> Result<(), String> {
    let failure = crate::launch::runner_create_409().await?;
    if failure.error().is_empty() {
        return Err("The production runner create path returned no failure.".to_owned());
    }
    let reservation_id = failure.reservation_id();
    let scope = failure.scope().clone();
    let receipt = scope
        .bootstrap_container_create_terminal_response(
            reservation_id,
            crate::launch::bootstrap::BootstrapContainerRole::Runner,
        )
        .await
        .map_err(|_| "The journal could not read the Runner create response.".to_owned())?
        .ok_or_else(|| "The journal did not retain the Runner create response.".to_owned())?;
    assert_eq!(receipt.status_code(), 409);
    assert_eq!(receipt.client_api_version(), (1, 53));

    let claim = scope
        .claim_pre_jit_cleanup(reservation_id)
        .await
        .map_err(|_| "The journal could not claim pre-JIT cleanup.".to_owned())?
        .ok_or_else(|| "The terminal Runner create did not permit cleanup.".to_owned())?;
    assert_eq!(claim.operation_id(), receipt.operation_id());
    assert_eq!(claim.engine_id(), receipt.engine_id());
    assert!(claim.lease_until() > 0);
    assert!(
        scope
            .validate_pre_jit_cleanup_claim(&claim)
            .await
            .map_err(|_| "The journal could not validate its cleanup claim.".to_owned())?
    );
    assert!(
        scope
            .claim_pre_jit_cleanup(reservation_id)
            .await
            .map_err(|_| "The journal could not check a competing cleanup claim.".to_owned())?
            .is_none()
    );

    let captured = failure.finish().await?;
    if captured.server_error.is_some()
        || captured
            .requests
            .iter()
            .filter(|request| {
                request.target.contains("/containers/create")
                    && request.response_status == Some(409)
            })
            .count()
            != 1
        || !captured.jit_input.is_empty()
    {
        return Err(format!(
            "The test server did not record one Runner 409 while JIT configuration was withheld. Error: {:?}; create requests: {:?}; JIT bytes: {}.",
            captured.server_error,
            captured
                .requests
                .iter()
                .filter(|request| request.target.contains("/containers/create"))
                .map(|request| (&request.target, request.response_status))
                .collect::<Vec<_>>(),
            captured.jit_input.len(),
        ));
    }

    Ok(())
}

#[tokio::test]
async fn claimed_pre_jit_recovery_accepts_progress_without_changing_reservation_identity()
-> Result<(), String> {
    let failure = crate::launch::runner_create_409().await?;
    let reservation_id = failure.reservation_id();
    let scope = failure.scope().clone();
    let claim = scope
        .claim_pre_jit_cleanup(reservation_id)
        .await
        .map_err(|_| "The journal could not claim pre-JIT cleanup.".to_owned())?
        .ok_or_else(|| "The terminal Runner create did not permit cleanup.".to_owned())?;

    crate::journal::connection::storage::in_transaction(scope.journal_ref(), async |connection| {
        let updated = connection
            .execute(
                "UPDATE capacity_reservation_pre_jit_cleanup_progress SET phase = 'bootstrap_absent', evidence_sha256 = ?1 WHERE reservation_id = ?2 AND claim_generation = ?3 AND phase = 'claimed'",
                (claim.snapshot_sha256(), reservation_id, claim.generation()),
            )
            .await
            .map_err(|_| crate::error::HostError::Journal)?;
        if updated != 1 {
            return Err(crate::error::HostError::Journal);
        }
        Ok(())
    })
    .await
    .map_err(|_| "The active claim did not accept the source state for its phase.".to_owned())?;

    if !scope
        .validate_pre_jit_cleanup_claim(&claim)
        .await
        .map_err(|_| "The journal could not validate its progressed cleanup claim.".to_owned())?
    {
        return Err("The active claim became invalid after cleanup progress.".to_owned());
    }
    crate::journal::connection::storage::in_transaction(scope.journal_ref(), async |connection| {
        for phase in ["initializer_absent", "storage_absent", "network_absent", "settled"] {
            let updated = connection
                .execute(
                    "UPDATE capacity_reservation_pre_jit_cleanup_progress SET phase = ?1, evidence_sha256 = ?2 WHERE reservation_id = ?3 AND claim_generation = ?4",
                    (phase, claim.snapshot_sha256(), reservation_id, claim.generation()),
                )
                .await
                .map_err(|_| crate::error::HostError::Journal)?;
            if updated != 1 {
                return Err(crate::error::HostError::Journal);
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| "The progress record rejected its ordered completion stages.".to_owned())?;
    if scope
        .validate_pre_jit_cleanup_claim(&claim)
        .await
        .map_err(|_| "The journal could not validate a terminal cleanup claim.".to_owned())?
    {
        return Err("The settled claim remained available for more effects.".to_owned());
    }
    if scope
        .claim_pre_jit_cleanup(reservation_id)
        .await
        .map_err(|_| "The journal could not reject a settled cleanup claim.".to_owned())?
        .is_some()
    {
        return Err("The journal reissued a settled cleanup claim.".to_owned());
    }
    let _captured = failure.finish().await?;
    Ok(())
}
