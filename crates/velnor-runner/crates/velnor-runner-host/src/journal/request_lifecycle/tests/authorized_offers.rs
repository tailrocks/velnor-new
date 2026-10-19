use crate::error::HostError;
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, AuthorizedOffer, CapacityLimits, Journal,
    SourceAuthorizationResult,
};

use super::helpers::{Scratch, binding, created_scope, receipt, source};

async fn setup(
    id: &str,
    repository_id: i64,
    set_id: i64,
) -> Result<(Scratch, Journal, crate::journal::ConnectionJournal), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let scope = created_scope(&journal, &binding(id, repository_id)?, set_id).await?;
    Ok((scratch, journal, scope))
}

async fn record_offer(
    scope: &crate::journal::ConnectionJournal,
    request_id: i64,
    message_id: i64,
    repository_id: i64,
) -> Result<(), HostError> {
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await?;
    let receipt = receipt(scope, request_id, message_id, repository_id).await?;
    if scope
        .record_source_authorization(&identity, &receipt)
        .await?
        != SourceAuthorizationResult::Recorded
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

#[tokio::test]
async fn authorized_offer_preserves_message_zero_across_reopen() -> Result<(), HostError> {
    let (scratch, journal, scope) = setup("offer-reopen", 101, 501).await?;
    record_offer(&scope, 801, 0, 101).await?;
    let offer = scope
        .authorized_offer(801, 0)
        .await?
        .ok_or(HostError::Journal)?;
    assert_offer(&offer, &scope, 801, 0);
    let original_json = offer.source_json().to_owned();
    let original_digest = offer.source_digest().to_owned();
    drop(offer);
    drop(scope);
    drop(journal);

    let journal = Journal::open(&scratch.file()).await?;
    let scope = journal
        .connection_scope(&binding("offer-reopen", 101)?)
        .await?;
    let replayed = scope.authorized_offers(8).await?;
    assert_eq!(replayed.len(), 1);
    assert_offer(&replayed[0], &scope, 801, 0);
    assert_eq!(replayed[0].source_json(), original_json);
    assert_eq!(replayed[0].source_digest(), original_digest);
    Ok(())
}

#[tokio::test]
async fn same_source_new_envelope_uses_original_offer_and_rejects_changes() -> Result<(), HostError>
{
    let (_scratch, _journal, scope) = setup("offer-envelope", 101, 501).await?;
    record_offer(&scope, 831, 1_031, 101).await?;
    let repeated = receipt(&scope, 831, 1_032, 101).await?;
    assert_eq!(
        scope
            .record_source_authorization(
                &scope
                    .assignment_identity(AssignmentKey::AssignedRequest { request_id: 831 })
                    .await?,
                &repeated,
            )
            .await?,
        SourceAuthorizationResult::Replayed
    );
    let offer = scope
        .authorized_offer_for_receipt(&repeated)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(offer.request_id(), 831);
    assert_eq!(offer.message_id(), 1_031);
    assert_eq!(repeated.message_id(), 1_032);

    let binding = scope.source_authorization_binding().await?;
    let changed = crate::launch::steps::source::test_receipt(
        &binding,
        831,
        1_033,
        source(101, 9_999),
        crate::config::SourcePolicyMode::Repository,
    );
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 831 })
        .await?;
    assert_eq!(
        scope.record_source_authorization(&identity, &changed).await,
        Err(HostError::Identity)
    );
    assert_eq!(
        scope.authorized_offer_for_receipt(&changed).await,
        Err(HostError::Identity)
    );

    assert!(scope.claim_request_acquire(&identity).await?);
    assert!(
        scope
            .authorized_offer_for_receipt(&repeated)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn enumeration_is_bounded_and_ordered_by_message_then_request() -> Result<(), HostError> {
    let (_scratch, _journal, scope) = setup("offer-order", 101, 501).await?;
    record_offer(&scope, 803, 9, 101).await?;
    record_offer(&scope, 802, 0, 101).await?;
    record_offer(&scope, 801, 4, 101).await?;
    let offers = scope.authorized_offers(2).await?;
    assert_eq!(offers.len(), 2);
    assert_eq!(
        offers
            .iter()
            .map(|offer| (offer.message_id(), offer.request_id()))
            .collect::<Vec<_>>(),
        [(0, 802), (4, 801)]
    );
    assert!(scope.authorized_offers(0).await.is_err());
    assert!(scope.authorized_offers(257).await.is_err());
    let exact = scope
        .authorized_offer(803, 9)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(exact.request_id(), 803);
    Ok(())
}

#[tokio::test]
async fn requested_uncertain_and_acquired_rows_are_not_offers() -> Result<(), HostError> {
    let (_scratch, _journal, scope) = setup("offer-states", 101, 501).await?;
    for request_id in [901, 902, 903] {
        record_offer(&scope, request_id, request_id + 100, 101).await?;
    }
    let requested = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 901 })
        .await?;
    assert!(scope.claim_request_acquire(&requested).await?);

    let uncertain = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 902 })
        .await?;
    assert!(scope.claim_request_acquire(&uncertain).await?);
    scope
        .record_request_acquire(&uncertain, AssignedAcquireResult::Uncertain)
        .await?;
    let uncertain_receipt = receipt(&scope, 902, 1_002, 101).await?;
    assert!(
        scope
            .authorized_offer_for_receipt(&uncertain_receipt)
            .await?
            .is_none()
    );

    let acquired = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 903 })
        .await?;
    assert!(scope.claim_request_acquire(&acquired).await?);
    scope
        .record_request_acquire(&acquired, AssignedAcquireResult::Returned(vec![903]))
        .await?;
    let acquired_receipt = receipt(&scope, 903, 1_003, 101).await?;
    assert!(
        scope
            .authorized_offer_for_receipt(&acquired_receipt)
            .await?
            .is_none()
    );

    assert!(scope.authorized_offers(8).await?.is_empty());
    for (request_id, message_id) in [(901, 1_001), (902, 1_002), (903, 1_003)] {
        assert!(
            scope
                .authorized_offer(request_id, message_id)
                .await?
                .is_none()
        );
    }
    Ok(())
}

#[tokio::test]
async fn offers_are_isolated_by_live_connection_and_scale_set() -> Result<(), HostError> {
    let (_scratch, journal, first) = setup("offer-first", 101, 501).await?;
    let binding_second = binding("offer-second", 202)?;
    let second = created_scope(&journal, &binding_second, 502).await?;
    record_offer(&first, 911, 1_011, 101).await?;
    record_offer(&second, 911, 1_011, 202).await?;

    let first_rows = first.authorized_offers(8).await?;
    let second_rows = second.authorized_offers(8).await?;
    assert_eq!(first_rows.len(), 1);
    assert_eq!(second_rows.len(), 1);
    assert_ne!(
        first_rows[0].connection_id(),
        second_rows[0].connection_id()
    );
    assert_ne!(first_rows[0].source_json(), second_rows[0].source_json());
    assert!(second.authorized_offer(911, 1_012).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn malformed_durable_source_digest_fails_closed() -> Result<(), HostError> {
    let (_scratch, _journal, scope) = setup("offer-corrupt", 101, 501).await?;
    record_offer(&scope, 921, 1_021, 101).await?;
    crate::journal::connection::storage::in_transaction(scope.journal_ref(), async |connection| {
        connection
            .execute(
                "UPDATE connection_request_authorizations SET source_digest = ?1 WHERE connection_id = ?2 AND scale_set_id = ?3 AND request_id = ?4",
                ("f".repeat(64), scope.connection_id().as_str(), 501_i64, 921_i64),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await?;
    assert!(scope.authorized_offers(8).await.is_err());
    assert!(scope.authorized_offer(921, 1_021).await.is_err());
    Ok(())
}

fn assert_offer(
    offer: &AuthorizedOffer,
    scope: &crate::journal::ConnectionJournal,
    request_id: i64,
    message_id: i64,
) {
    assert_eq!(offer.connection_id(), scope.connection_id());
    assert_eq!(offer.scale_set_id(), 501);
    assert_eq!(offer.request_id(), request_id);
    assert_eq!(offer.message_id(), message_id);
    assert_eq!(
        offer.policy_mode(),
        crate::config::SourcePolicyMode::Repository
    );
    assert_eq!(offer.binding_digest(), scope.binding_digest());
    assert_eq!(offer.source_digest().len(), 64);
    assert!(!offer.source_json().is_empty());
    assert_eq!(
        offer
            .assignment_identity()
            .expect("request identity")
            .request_id(),
        Some(request_id)
    );
}
