//! Atomic bounded selection for multi-Available messages.

use std::num::NonZeroU32;

use crate::journal::{BatchCapacityClaim, BatchOfferState, Journal};
use crate::{HostError, IntentState};

use crate::journal::tests::Scratch;

#[tokio::test]
async fn reservations_select_only_free_slots_atomically_and_survive_reopen() -> Result<(), String> {
    let scratch = Scratch::new("capacity-batch").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let limit = NonZeroU32::new(2).ok_or("nonzero limit")?;
    let BatchCapacityClaim::Offers(claims) = journal
        .reserve_launch_batch_if_accepting(77, &[3, 4, 5], 3, limit)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("batch was blocked by drain".to_owned());
    };
    assert_eq!(
        claims
            .iter()
            .map(|claim| claim.request_id)
            .collect::<Vec<_>>(),
        [3, 4, 5]
    );
    let ids = claims
        .iter()
        .filter_map(|claim| match claim.state {
            BatchOfferState::Reserved { launch_id } => Some(launch_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert_eq!(claims[2].state, BatchOfferState::Deferred);

    let BatchCapacityClaim::Offers(replay) = journal
        .reserve_launch_batch_if_accepting(77, &[3, 4, 5], 3, limit)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("replay was blocked by drain".to_owned());
    };
    assert_eq!(
        replay[0].state,
        BatchOfferState::Ready { launch_id: ids[0] }
    );
    assert_eq!(
        replay[1].state,
        BatchOfferState::Ready { launch_id: ids[1] }
    );
    assert_eq!(replay[2].state, BatchOfferState::Deferred);

    for id in &ids {
        journal
            .record_launch_effect_intent(*id)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(
            journal.launch_effect_state(*id).await,
            Ok(crate::journal::LaunchEffectState::MayHaveEffect)
        );
    }
    drop(journal);

    let reopened = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let BatchCapacityClaim::Offers(replayed) = reopened
        .reserve_launch_batch_if_accepting(77, &[3, 4, 5], 3, limit)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("reopen was blocked by drain".to_owned());
    };
    assert_eq!(
        replayed.iter().map(|claim| claim.state).collect::<Vec<_>>(),
        [
            BatchOfferState::Existing { launch_id: ids[0] },
            BatchOfferState::Existing { launch_id: ids[1] },
            BatchOfferState::Deferred,
        ]
    );
    let rows = reopened.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.state == IntentState::Pending));
    assert!(rows.iter().all(|row| row.runner_request_id.is_none()));
    Ok(())
}

#[tokio::test]
async fn single_and_multi_offer_paths_share_exact_replay_subjects() -> Result<(), String> {
    let scratch = Scratch::new("capacity-batch-cross-replay").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let limit = NonZeroU32::new(4).ok_or("nonzero limit")?;
    let (legacy_id, fresh) = journal
        .begin_launch("m81r5")
        .await
        .map_err(|error| error.to_string())?;
    assert!(fresh);
    let BatchCapacityClaim::Offers(legacy_claims) = journal
        .reserve_launch_batch_if_accepting(81, &[5, 6], 2, limit)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("batch was blocked by drain".to_owned());
    };
    assert_eq!(
        legacy_claims[0].state,
        BatchOfferState::Existing {
            launch_id: legacy_id
        }
    );
    assert!(matches!(
        legacy_claims[1].state,
        BatchOfferState::Reserved { .. }
    ));

    let (id, fresh) = journal
        .begin_launch("m81r6")
        .await
        .map_err(|error| error.to_string())?;
    assert!(!fresh);
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(
        rows.iter()
            .find(|row| row.subject == "m81r6")
            .map(|row| row.id),
        Some(id)
    );
    assert_eq!(rows.len(), 2);
    Ok(())
}

#[tokio::test]
async fn bounded_offer_reservation_obeys_drain_before_any_external_effect() -> Result<(), String> {
    let scratch = Scratch::new("capacity-batch-drain").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .reserve_launch_batch_if_accepting(
                91,
                &[1, 2],
                2,
                NonZeroU32::new(2).ok_or("nonzero limit")?,
            )
            .await
            .map_err(|error| error.to_string())?,
        BatchCapacityClaim::Draining
    );
    assert_eq!(
        journal.rows().await.map_err(|error| error.to_string())?,
        Vec::new()
    );
    Ok(())
}

#[tokio::test]
async fn invalid_batch_ids_fail_without_rows() -> Result<(), String> {
    let scratch = Scratch::new("capacity-batch-invalid").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let limit = NonZeroU32::new(4).ok_or("nonzero limit")?;
    for ids in [&[][..], &[0][..], &[4, 4][..], &[1, 2][..]] {
        let max_new = if ids.len() == 2 { 3 } else { ids.len() };
        assert_eq!(
            journal
                .reserve_launch_batch_if_accepting(92, ids, max_new, limit)
                .await,
            Err(HostError::Journal)
        );
    }
    assert_eq!(
        journal.rows().await.map_err(|error| error.to_string())?,
        Vec::new()
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_batches_cannot_reserve_more_than_global_capacity() -> Result<(), String> {
    use std::sync::Arc;

    let scratch = Scratch::new("capacity-batch-concurrent").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let first = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let second = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let maximum = NonZeroU32::new(2).ok_or("nonzero limit")?;
    let first_reservation = reserve_after(first, 101, vec![1, 2], barrier.clone(), maximum);
    let second_reservation = reserve_after(second, 102, vec![3, 4], barrier.clone(), maximum);
    let start = barrier.wait();
    let (first_result, second_result, _) =
        tokio::join!(first_reservation, second_reservation, start);

    let reserved = [&first_result, &second_result]
        .into_iter()
        .filter_map(|result| match result {
            Ok(BatchCapacityClaim::Offers(claims)) => Some(
                claims
                    .iter()
                    .filter(|claim| matches!(claim.state, BatchOfferState::Reserved { .. }))
                    .count(),
            ),
            Ok(BatchCapacityClaim::Draining) | Err(_) => None,
        })
        .sum::<usize>();
    assert_eq!(
        reserved, 2,
        "one transaction should reserve both free permits"
    );

    for result in [&first_result, &second_result] {
        if let Ok(BatchCapacityClaim::Offers(claims)) = result {
            assert!(claims.iter().all(|claim| matches!(
                claim.state,
                BatchOfferState::Reserved { .. } | BatchOfferState::Deferred
            )));
        }
    }
    let rows = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?
        .rows()
        .await
        .map_err(|error| error.to_string())?;
    let occupied = rows
        .iter()
        .filter(|row| row.kind == "launch" && row.state != IntentState::Failed)
        .count();
    assert_eq!(
        occupied, 2,
        "batch reservations must fill but not exceed capacity"
    );
    assert!(!matches!(first_result, Ok(BatchCapacityClaim::Draining)));
    assert!(!matches!(second_result, Ok(BatchCapacityClaim::Draining)));
    Ok(())
}

async fn reserve_after(
    journal: Journal,
    message_id: i64,
    request_ids: Vec<i64>,
    barrier: std::sync::Arc<tokio::sync::Barrier>,
    maximum: NonZeroU32,
) -> Result<BatchCapacityClaim, HostError> {
    barrier.wait().await;
    journal
        .reserve_launch_batch_if_accepting(message_id, &request_ids, request_ids.len(), maximum)
        .await
}
