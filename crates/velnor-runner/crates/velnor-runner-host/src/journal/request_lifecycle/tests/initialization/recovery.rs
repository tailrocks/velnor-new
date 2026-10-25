use crate::error::HostError;
use crate::journal::connection::storage::read_transaction;
use crate::journal::{ConnectionJournal, Journal, StorageInitializationClaim};

use super::super::admission::JOB_LIMIT;
use super::super::helpers::{Scratch, binding, created_scope, test_initializer_limits};

#[tokio::test]
async fn uncertain_initializer_recovery_requires_exact_absence_and_stays_charged()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-abort-recovery", 947)?, 1947).await?;
    let reservation_id = super::super::helpers::prepare_physical_initializer(&scope, 'b').await?;
    let images = crate::launch::verified_images::test_worker_images();
    let image =
        crate::worker::storage_initialization::StorageInitializationImage::from_verified_release(
            &images,
        )?;
    let claim = scope
        .claim_storage_initialization(reservation_id, &image, test_initializer_limits()?)
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .mark_storage_initializer_create_uncertain(&claim)
        .await?;

    let recovery = claim_abort(&scope, reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(claim_abort(&scope, reservation_id).await?.is_none());
    scope
        .mark_storage_initializer_abort_uncertain(&recovery)
        .await?;
    let stale = recovery;
    let recovery = claim_abort(&scope, reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        scope
            .mark_storage_initializer_abort_uncertain(&stale)
            .await
            .is_err()
    );
    let stale_proof =
        crate::worker::storage_initialization::VerifiedStorageInitializerAbsent::test_absent(
            &stale,
        );
    assert!(
        scope
            .record_storage_initializer_absent(&stale, &stale_proof)
            .await
            .is_err()
    );
    let proof =
        crate::worker::storage_initialization::VerifiedStorageInitializerAbsent::test_absent(
            &recovery,
        );
    scope
        .record_storage_initializer_absent(&recovery, &proof)
        .await?;
    assert!(claim_abort(&scope, reservation_id).await?.is_none());

    let reservation_state = Box::pin(read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection
            .query(
                "SELECT state FROM capacity_reservations WHERE id = ?1",
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<String>(0)
            .map_err(|_| HostError::Journal)
    }))
    .await?;
    assert_eq!(reservation_state, "uncertain");
    Ok(())
}

#[tokio::test]
async fn unresolved_dispatched_create_cannot_be_released_after_a_missing_helper_check()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-create-unknown", 948)?, 1948).await?;
    let reservation_id = super::super::helpers::prepare_physical_initializer(&scope, 'c').await?;
    let images = crate::launch::verified_images::test_worker_images();
    let image =
        crate::worker::storage_initialization::StorageInitializationImage::from_verified_release(
            &images,
        )?;
    let claim = scope
        .claim_storage_initialization(reservation_id, &image, test_initializer_limits()?)
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .record_storage_initializer_create_dispatched(&claim)
        .await?;
    scope
        .mark_storage_initializer_create_outcome_unknown(&claim)
        .await?;
    let recovery = claim_abort(&scope, reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let missing_helper =
        crate::worker::storage_initialization::VerifiedStorageInitializerAbsent::test_absent(
            &recovery,
        );
    assert!(
        scope
            .record_storage_initializer_absent(&recovery, &missing_helper)
            .await
            .is_err()
    );
    assert_eq!(
        super::super::helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "requested"
    );
    scope
        .mark_storage_initializer_abort_uncertain(&recovery)
        .await?;
    assert_eq!(
        super::super::helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "uncertain"
    );
    let state = Box::pin(read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection
            .query(
                "SELECT state FROM capacity_reservations WHERE id = ?1",
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        rows.next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<String>(0)
            .map_err(|_| HostError::Journal)
    }))
    .await?;
    assert_eq!(state, "uncertain");
    Ok(())
}

#[tokio::test]
async fn dispatched_create_reopens_as_charged_until_explicit_recovery_claim()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("storage-dispatch-reopen", 950)?;
    let scope = created_scope(&journal, &binding, 1950).await?;
    let reservation_id = super::super::helpers::prepare_physical_initializer(&scope, 'e').await?;
    let images = crate::launch::verified_images::test_worker_images();
    let image =
        crate::worker::storage_initialization::StorageInitializationImage::from_verified_release(
            &images,
        )?;
    let claim = scope
        .claim_storage_initialization(reservation_id, &image, test_initializer_limits()?)
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .record_storage_initializer_create_dispatched(&claim)
        .await?;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&scratch.file()).await?;
    let scope = journal.connection_scope(&binding).await?;
    let before = initializer_recovery_state(&scope, reservation_id).await?;
    assert_eq!(
        before,
        (
            "reserved".to_owned(),
            "pending".to_owned(),
            "dispatched".to_owned(),
            "requested".to_owned(),
            "not_requested".to_owned()
        )
    );
    let candidate = scope
        .storage_initializer_abort_candidate(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let recovery = scope
        .claim_storage_initializer_abort(&candidate)
        .await?
        .ok_or(HostError::Journal)?;
    let after = initializer_recovery_state(&scope, reservation_id).await?;
    assert_eq!(
        after,
        (
            "uncertain".to_owned(),
            "uncertain".to_owned(),
            "outcome_unknown".to_owned(),
            "uncertain".to_owned(),
            "requested".to_owned()
        )
    );
    assert!(
        scope
            .claim_storage_initializer_abort(&candidate)
            .await?
            .is_none()
    );
    scope
        .mark_storage_initializer_abort_uncertain(&recovery)
        .await?;
    Ok(())
}

#[tokio::test]
async fn migration_rejects_a_different_abort_helper_id() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("storage-helper-id-binding", 949)?;
    let scope = created_scope(&journal, &binding, 1949).await?;
    let known_id = "a".repeat(64);
    let reservation_id = super::super::helpers::prepare_physical_initializer_with_known_helper(
        &scope, 'd', &known_id,
    )
    .await?;
    assert!(claim_abort(&scope, reservation_id).await?.is_some());
    let path = scratch.file();
    drop(scope);
    drop(journal);

    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mismatch = "b".repeat(64);
    assert!(connection
        .execute(
            "UPDATE capacity_reservation_storage_initialization SET abort_helper_id = ?2, abort_state = 'remove_requested' WHERE reservation_id = ?1",
            (reservation_id, mismatch.as_str()),
        )
        .await
        .is_err());
    connection
        .execute(
            "DROP TRIGGER storage_initialization_abort_id_format_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE capacity_reservation_storage_initialization SET abort_helper_id = ?2, abort_claim_generation = 1, abort_lease_until = 2147483647, abort_state = 'remove_requested' WHERE reservation_id = ?1",
            (reservation_id, mismatch),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 19", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

pub(super) async fn claim_abort(
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<Option<StorageInitializationClaim>, HostError> {
    let Some(candidate) = scope
        .storage_initializer_abort_candidate(reservation_id)
        .await?
    else {
        return Ok(None);
    };
    scope.claim_storage_initializer_abort(&candidate).await
}

pub(super) async fn initializer_recovery_state(
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<(String, String, String, String, String), HostError> {
    read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection.query(
            "SELECT reservation.state, intent.state, init.create_dispatch_state, init.create_state, init.abort_state FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id JOIN capacity_reservation_storage_initialization AS init ON init.reservation_id = reservation.id WHERE reservation.id = ?1",
            [reservation_id],
        ).await.map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        Ok((
            row.get(0).map_err(|_| HostError::Journal)?,
            row.get(1).map_err(|_| HostError::Journal)?,
            row.get(2).map_err(|_| HostError::Journal)?,
            row.get(3).map_err(|_| HostError::Journal)?,
            row.get(4).map_err(|_| HostError::Journal)?,
        ))
    }).await
}
