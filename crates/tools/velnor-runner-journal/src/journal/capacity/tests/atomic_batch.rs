//! All-or-none queue-offer reservation tests.

use std::collections::BTreeSet;
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::journal::tests::Scratch;
use crate::journal::{AtomicBatchCapacityClaim, BatchOfferState, CapacityClaim, Journal};
use crate::{HostError, IntentState};

#[tokio::test]
async fn insufficient_global_capacity_adds_no_rows_for_any_fresh_offer() -> Result<(), String> {
    let scratch = Scratch::new("capacity-atomic-full").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let held = super::admission::identity(
        "https://api.github.com",
        "other-repository",
        "other-session",
        201,
        1,
    )
    .map_err(|error| error.to_string())?;
    let CapacityClaim::New(held_id) = journal
        .reserve_launch_if_accepting(&held, NonZeroU32::new(2).ok_or("nonzero maximum")?)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("held launch row was not newly reserved".to_owned());
    };
    journal
        .record_launch_effect_intent(held_id)
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(
        journal
            .reserve_launch_batch_all_or_none_if_accepting(
                202,
                &[7, 8],
                NonZeroU32::new(2).ok_or("nonzero maximum")?,
            )
            .await
            .map_err(|error| error.to_string())?,
        AtomicBatchCapacityClaim::CapacityFull {
            occupied: 1,
            required: 2,
            available: 1,
            maximum: NonZeroU32::new(2).ok_or("nonzero maximum")?,
        }
    );

    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, held_id);
    assert!(rows[0].subject.starts_with("scope-v1:"));
    assert_eq!(
        rows[0].launch_effect,
        crate::journal::LaunchEffectState::MayHaveEffect
    );
    assert_eq!(rows[0].state, IntentState::Pending);
    Ok(())
}

#[tokio::test]
async fn exact_batch_replay_is_idempotent_without_duplicate_rows() -> Result<(), String> {
    let scratch = Scratch::new("capacity-atomic-replay").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero maximum")?;
    let AtomicBatchCapacityClaim::Offers(first) = journal
        .reserve_launch_batch_all_or_none_if_accepting(301, &[4, 5], maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("initial batch was not admitted".to_owned());
    };
    let ids = first
        .iter()
        .map(|claim| match claim.state {
            BatchOfferState::Reserved { launch_id } => Ok(launch_id),
            _ => Err("fresh offer was not reserved".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(ids.len(), 2);
    assert_eq!(
        journal
            .reserve_launch_batch_all_or_none_if_accepting(301, &[4, 4], maximum)
            .await,
        Err(HostError::Journal),
        "one batch cannot claim the same exact message/request identity twice"
    );

    let AtomicBatchCapacityClaim::Offers(replay) = journal
        .reserve_launch_batch_all_or_none_if_accepting(301, &[4, 5], maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("exact replay was not returned".to_owned());
    };
    assert_eq!(
        replay.iter().map(|claim| claim.state).collect::<Vec<_>>(),
        [
            BatchOfferState::Ready { launch_id: ids[0] },
            BatchOfferState::Ready { launch_id: ids[1] }
        ]
    );
    journal
        .record_launch_effect_intent(ids[0])
        .await
        .map_err(|error| error.to_string())?;
    let AtomicBatchCapacityClaim::Offers(after_effect) = journal
        .reserve_launch_batch_all_or_none_if_accepting(301, &[4, 5], maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("exact existing replay should remain observable".to_owned());
    };
    assert_eq!(
        after_effect
            .iter()
            .map(|claim| claim.state)
            .collect::<Vec<_>>(),
        [
            BatchOfferState::Existing { launch_id: ids[0] },
            BatchOfferState::Ready { launch_id: ids[1] }
        ]
    );
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        2
    );
    drop(journal);

    let reopened = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    let rows = reopened.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.iter().map(|row| row.id).collect::<Vec<_>>(), ids);
    Ok(())
}

#[tokio::test]
async fn drain_blocks_new_batch_but_preserves_exact_replay() -> Result<(), String> {
    let scratch = Scratch::new("capacity-atomic-drain").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero maximum")?;
    let AtomicBatchCapacityClaim::Offers(first) = journal
        .reserve_launch_batch_all_or_none_if_accepting(302, &[4, 5], maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("initial batch was not admitted".to_owned());
    };
    let ids = first
        .iter()
        .map(|claim| match claim.state {
            BatchOfferState::Reserved { launch_id } => Ok(launch_id),
            _ => Err("fresh offer was not reserved".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;

    let AtomicBatchCapacityClaim::Offers(replay) = journal
        .reserve_launch_batch_all_or_none_if_accepting(302, &[4, 5], maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("exact replay should remain observable during drain".to_owned());
    };
    assert_eq!(
        replay.iter().map(|claim| claim.state).collect::<Vec<_>>(),
        [
            BatchOfferState::Ready { launch_id: ids[0] },
            BatchOfferState::Ready { launch_id: ids[1] }
        ]
    );
    assert_eq!(
        journal
            .reserve_launch_batch_all_or_none_if_accepting(302, &[6], maximum)
            .await
            .map_err(|error| error.to_string())?,
        AtomicBatchCapacityClaim::Draining
    );
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn second_insert_failure_rolls_back_the_entire_batch() -> Result<(), String> {
    let scratch = Scratch::new("capacity-atomic-rollback").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let conn = journal
        .connection()
        .await
        .map_err(|error| error.to_string())?;
    conn.execute(
        "CREATE TRIGGER reject_second_atomic_offer BEFORE INSERT ON intents WHEN NEW.subject = 'm401r2' BEGIN SELECT RAISE(ABORT, 'injected batch insert fault'); END",
        (),
    )
    .await
    .map_err(|error| error.to_string())?;

    assert_eq!(
        journal
            .reserve_launch_batch_all_or_none_if_accepting(
                401,
                &[1, 2],
                NonZeroU32::new(2).ok_or("nonzero maximum")?,
            )
            .await,
        Err(HostError::Journal)
    );
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert!(
        rows.is_empty(),
        "first insert must roll back with the second"
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_batches_are_complete_or_rejected_without_oversubscription() -> Result<(), String>
{
    let scratch = Scratch::new("capacity-atomic-concurrent").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let first = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let second = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let maximum = NonZeroU32::new(3).ok_or("nonzero maximum")?;
    let first_reservation = reserve_after(first, 501, vec![1, 2], barrier.clone(), maximum);
    let second_reservation = reserve_after(second, 502, vec![3, 4], barrier.clone(), maximum);
    let start = barrier.wait();
    let (first_result, second_result, _) =
        tokio::join!(first_reservation, second_reservation, start);
    let results = [first_result, second_result];
    let successful_batches = results
        .iter()
        .filter(|result| matches!(result, Ok(AtomicBatchCapacityClaim::Offers(_))))
        .count();
    assert_eq!(successful_batches, 1, "one whole two-row batch should win");
    for result in &results {
        if let Ok(AtomicBatchCapacityClaim::Offers(claims)) = result {
            assert_eq!(claims.len(), 2);
            assert!(claims.iter().all(|claim| matches!(
                claim.state,
                BatchOfferState::Reserved { .. }
                    | BatchOfferState::Ready { .. }
                    | BatchOfferState::Existing { .. }
            )));
        }
        if let Ok(AtomicBatchCapacityClaim::CapacityFull {
            required,
            available,
            ..
        }) = result
        {
            assert_eq!(*required, 2);
            assert!(*available < *required);
        }
    }
    let rows = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?
        .rows()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.state == IntentState::Pending));
    let subjects = rows
        .iter()
        .map(|row| row.subject.as_str())
        .collect::<BTreeSet<_>>();
    assert!(
        subjects == ["m501r1", "m501r2"].into_iter().collect()
            || subjects == ["m502r3", "m502r4"].into_iter().collect(),
        "one complete batch should win the serialized capacity race"
    );
    Ok(())
}

#[tokio::test]
async fn simultaneous_exact_replays_return_the_same_durable_request_ids() -> Result<(), String> {
    let scratch = Scratch::new("capacity-atomic-same-replay").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let first = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let second = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let maximum = NonZeroU32::new(2).ok_or("nonzero maximum")?;
    let first_reservation = reserve_after(first, 503, vec![4, 5], barrier.clone(), maximum);
    let second_reservation = reserve_after(second, 503, vec![4, 5], barrier.clone(), maximum);
    let start = barrier.wait();
    let (first_result, second_result, _) =
        tokio::join!(first_reservation, second_reservation, start);
    let (Ok(AtomicBatchCapacityClaim::Offers(first)), Ok(AtomicBatchCapacityClaim::Offers(second))) =
        (first_result, second_result)
    else {
        return Err("simultaneous exact replay was not accepted".to_owned());
    };
    let launch_ids = |claims: &[crate::journal::BatchOfferClaim]| {
        claims
            .iter()
            .map(|claim| match claim.state {
                BatchOfferState::Reserved { launch_id }
                | BatchOfferState::Ready { launch_id }
                | BatchOfferState::Existing { launch_id } => Ok(launch_id),
                BatchOfferState::Deferred => Err("all-or-none replay returned Deferred".to_owned()),
            })
            .collect::<Result<Vec<_>, _>>()
    };
    assert_eq!(
        first
            .iter()
            .map(|claim| claim.request_id)
            .collect::<Vec<_>>(),
        [4, 5]
    );
    assert_eq!(
        second
            .iter()
            .map(|claim| claim.request_id)
            .collect::<Vec<_>>(),
        [4, 5]
    );
    let first_ids = launch_ids(&first)?;
    let second_ids = launch_ids(&second)?;
    assert_eq!(first_ids, second_ids, "both calls must name the same rows");
    let rows = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?
        .rows()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter()
            .map(|row| row.subject.as_str())
            .collect::<Vec<_>>(),
        ["m503r4", "m503r5"]
    );
    assert_eq!(rows.iter().map(|row| row.id).collect::<Vec<_>>(), first_ids);
    Ok(())
}

async fn reserve_after(
    journal: Journal,
    message_id: i64,
    request_ids: Vec<i64>,
    barrier: Arc<tokio::sync::Barrier>,
    maximum: NonZeroU32,
) -> Result<AtomicBatchCapacityClaim, HostError> {
    barrier.wait().await;
    journal
        .reserve_launch_batch_all_or_none_if_accepting(message_id, &request_ids, maximum)
        .await
}
