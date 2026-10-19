use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::in_transaction;

use super::super::admission::JOB_LIMIT;
use super::super::helpers::{Scratch, binding, created_scope, test_initializer_limits};

#[tokio::test]
async fn undispatched_create_reopens_as_charged_until_explicit_recovery_claim()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("storage-undispatched-reopen", 951)?;
    let scope = created_scope(&journal, &binding, 1951).await?;
    let reservation_id = super::super::helpers::prepare_physical_initializer(&scope, 'f').await?;
    let images = crate::launch::verified_images::test_worker_images();
    let image =
        crate::worker::storage_initialization::StorageInitializationImage::from_verified_release(
            &images,
        )?;
    scope
        .claim_storage_initialization(reservation_id, &image, test_initializer_limits()?)
        .await?
        .ok_or(HostError::Journal)?;
    let path = scratch.file();
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(
        super::recovery::initializer_recovery_state(&scope, reservation_id).await?,
        (
            "reserved".to_owned(),
            "pending".to_owned(),
            "not_dispatched".to_owned(),
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
    assert_eq!(
        super::recovery::initializer_recovery_state(&scope, reservation_id).await?,
        (
            "uncertain".to_owned(),
            "uncertain".to_owned(),
            "not_dispatched".to_owned(),
            "uncertain".to_owned(),
            "requested".to_owned()
        )
    );
    scope
        .mark_storage_initializer_abort_uncertain(&recovery)
        .await?;
    Ok(())
}

#[tokio::test]
async fn removal_intent_survives_reopen_and_abort_claim_recovery() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("storage-remove-intent-reopen", 952)?;
    let scope = created_scope(&journal, &binding, 1952).await?;
    let helper_id = "c".repeat(64);
    let reservation_id = super::super::helpers::prepare_physical_initializer_with_known_helper(
        &scope, 'a', &helper_id,
    )
    .await?;
    let claim = super::recovery::claim_abort(&scope, reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    Box::pin(in_transaction(scope.journal_ref(), async |connection| {
        let changed = connection
            .execute(
                "UPDATE capacity_reservation_storage_initialization SET abort_helper_id = helper_id, abort_state = 'remove_requested' WHERE reservation_id = ?1 AND connection_id = ?2 AND abort_claim_generation = ?3 AND abort_lease_until > 0 AND helper_id = ?4",
                (reservation_id, scope.connection_id().as_str(), claim.abort_claim_generation(), helper_id.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    }))
    .await?;
    let path = scratch.file();
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(
        super::super::helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "remove_requested"
    );
    scope
        .expire_storage_initializer_abort_lease(reservation_id)
        .await?;
    let candidate = scope
        .storage_initializer_abort_candidate(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let recovery = scope
        .claim_storage_initializer_abort(&candidate)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        super::super::helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "remove_requested"
    );
    assert_eq!(recovery.abort_helper_id(), Some(helper_id.as_str()));
    let proof =
        crate::worker::storage_initialization::VerifiedStorageInitializerAbsent::test_absent(
            &recovery,
        );
    scope
        .record_storage_initializer_absent(&recovery, &proof)
        .await?;
    assert_eq!(
        super::super::helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "absent"
    );
    Ok(())
}
