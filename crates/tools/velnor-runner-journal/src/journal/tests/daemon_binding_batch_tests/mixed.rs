//! Mixed replay states for scoped daemon-bound batch reservations.

use std::num::NonZeroU32;

use crate::Journal;
use crate::journal::{
    BoundBatchCapacityClaim, BoundCapacityClaim, CapacityClaim, JournalDockerDaemonBinding,
    ScopedLaunchIdentity,
};

use super::{Scratch, binding, offer};

struct ExistingMixedRows {
    matching: ScopedLaunchIdentity,
    matching_id: i64,
    unbound: ScopedLaunchIdentity,
    unbound_id: i64,
    changed: ScopedLaunchIdentity,
    changed_id: i64,
}

async fn reserve_existing_mixed_rows(
    journal: &Journal,
    current: &JournalDockerDaemonBinding,
    previous: &JournalDockerDaemonBinding,
    maximum: NonZeroU32,
) -> Result<ExistingMixedRows, String> {
    let matching = offer("widget-match", 41, "session-match").map_err(|error| error.to_string())?;
    let BoundCapacityClaim::New(matching_id) = journal
        .reserve_linux_launch_if_accepting(&matching, current, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("matching row was not newly reserved".to_owned());
    };

    let unbound =
        offer("widget-unbound", 42, "session-unbound").map_err(|error| error.to_string())?;
    let CapacityClaim::New(unbound_id) = journal
        .reserve_launch_if_accepting(&unbound, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("legacy row was not newly reserved".to_owned());
    };

    let changed =
        offer("widget-changed", 43, "session-changed").map_err(|error| error.to_string())?;
    let BoundCapacityClaim::New(changed_id) = journal
        .reserve_linux_launch_if_accepting(&changed, previous, maximum)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("previous-engine row was not newly reserved".to_owned());
    };

    Ok(ExistingMixedRows {
        matching,
        matching_id,
        unbound,
        unbound_id,
        changed,
        changed_id,
    })
}

async fn reserve_mixed_batch(
    journal: &Journal,
    old: &ExistingMixedRows,
    current: &JournalDockerDaemonBinding,
    maximum: NonZeroU32,
) -> Result<(i64, i64), String> {
    let fresh_first =
        offer("widget-first", 44, "session-first").map_err(|error| error.to_string())?;
    let fresh_last = offer("widget-last", 45, "session-last").map_err(|error| error.to_string())?;
    let BoundBatchCapacityClaim::Offers(claims) = journal
        .reserve_linux_launch_batch_all_or_none_if_accepting(
            &[
                fresh_first,
                old.matching.clone(),
                old.unbound.clone(),
                old.changed.clone(),
                fresh_last,
            ],
            current,
            maximum,
        )
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("two fresh rows should fit after three existing rows".to_owned());
    };
    let [
        BoundCapacityClaim::New(first_id),
        BoundCapacityClaim::Existing(matching_id),
        BoundCapacityClaim::ExistingUnbound(unbound_id),
        BoundCapacityClaim::ExistingBindingChanged(changed_id),
        BoundCapacityClaim::New(last_id),
    ] = claims.as_slice()
    else {
        return Err(format!("unexpected mixed claim order: {claims:?}"));
    };
    assert_eq!(*matching_id, old.matching_id);
    assert_eq!(*unbound_id, old.unbound_id);
    assert_eq!(*changed_id, old.changed_id);
    assert_ne!(first_id, last_id);
    Ok((*first_id, *last_id))
}

async fn assert_mixed_rows_keep_their_existing_binding_state(
    journal: &Journal,
    old: &ExistingMixedRows,
    current: &JournalDockerDaemonBinding,
    previous: &JournalDockerDaemonBinding,
    fresh_ids: (i64, i64),
) -> Result<(), String> {
    assert_eq!(
        journal
            .launch_daemon_binding(old.matching_id)
            .await
            .map_err(|error| error.to_string())?,
        Some(current.clone())
    );
    assert_eq!(
        journal
            .launch_daemon_binding(old.unbound_id)
            .await
            .map_err(|error| error.to_string())?,
        None
    );
    assert_eq!(
        journal
            .launch_daemon_binding(old.changed_id)
            .await
            .map_err(|error| error.to_string())?,
        Some(previous.clone())
    );
    for fresh_id in [fresh_ids.0, fresh_ids.1] {
        assert_eq!(
            journal
                .launch_daemon_binding(fresh_id)
                .await
                .map_err(|error| error.to_string())?,
            Some(current.clone())
        );
    }
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        5
    );
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .occupied_launches,
        5
    );
    Ok(())
}

async fn assert_mixed_batch_replay_is_idempotent(
    journal: &Journal,
    old: ExistingMixedRows,
    current: &JournalDockerDaemonBinding,
    maximum: NonZeroU32,
    fresh_ids: (i64, i64),
) -> Result<(), String> {
    let BoundBatchCapacityClaim::Offers(replayed) = journal
        .reserve_linux_launch_batch_all_or_none_if_accepting(
            &[
                offer("widget-first", 44, "session-first").map_err(|error| error.to_string())?,
                old.matching,
                old.unbound,
                old.changed,
                offer("widget-last", 45, "session-last").map_err(|error| error.to_string())?,
            ],
            current,
            maximum,
        )
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("mixed replay should return its row-level states".to_owned());
    };
    assert_eq!(
        replayed,
        vec![
            BoundCapacityClaim::Existing(fresh_ids.0),
            BoundCapacityClaim::Existing(old.matching_id),
            BoundCapacityClaim::ExistingUnbound(old.unbound_id),
            BoundCapacityClaim::ExistingBindingChanged(old.changed_id),
            BoundCapacityClaim::Existing(fresh_ids.1),
        ]
    );
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        5
    );
    Ok(())
}

#[tokio::test]
async fn mixed_batch_reports_row_states_and_reserves_only_fresh_capacity() -> Result<(), String> {
    let scratch = Scratch::new("bound-batch-mixed").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let current = binding().map_err(|error| error.to_string())?;
    let previous = JournalDockerDaemonBinding::new("/run/docker.sock", "engine-old")
        .map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(5).ok_or("nonzero maximum")?;
    let old = reserve_existing_mixed_rows(&journal, &current, &previous, maximum).await?;
    let fresh_ids = reserve_mixed_batch(&journal, &old, &current, maximum).await?;
    assert_mixed_rows_keep_their_existing_binding_state(
        &journal, &old, &current, &previous, fresh_ids,
    )
    .await?;
    assert_mixed_batch_replay_is_idempotent(&journal, old, &current, maximum, fresh_ids).await
}
