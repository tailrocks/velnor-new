use super::*;

#[test]
fn acknowledgment_requires_every_offer_and_side_effect_to_be_resolved() {
    let batch = available_batch();
    let empty = empty_ids();
    let completed = BTreeSet::from([23]);
    assert!(acknowledgement_allowed(
        &batch,
        state(true, false, &empty, &empty, &empty, &empty, &completed)
    ));
    assert!(!acknowledgement_allowed(
        &batch,
        state(true, false, &empty, &empty, &empty, &empty, &empty)
    ));

    assert!(!acknowledgement_allowed(
        &batch,
        state(true, true, &empty, &empty, &empty, &empty, &completed)
    ));
    assert!(!acknowledgement_allowed(
        &batch,
        state(
            true,
            false,
            &BTreeSet::from([23]),
            &empty,
            &empty,
            &empty,
            &completed
        )
    ));
    assert!(!acknowledgement_allowed(
        &batch,
        state(
            true,
            false,
            &empty,
            &BTreeSet::from([23]),
            &empty,
            &empty,
            &completed
        )
    ));
    assert!(!acknowledgement_allowed(
        &batch,
        state(
            true,
            false,
            &empty,
            &empty,
            &BTreeSet::from([23]),
            &empty,
            &completed
        )
    ));
    assert!(!acknowledgement_allowed(
        &batch,
        state(false, false, &empty, &empty, &empty, &empty, &completed)
    ));
}

#[test]
fn acknowledgment_rejects_unsupported_message_and_negative_cursor() {
    let mut batch = available_batch();
    batch.jobs[0].kind = InnerKind::Unsupported("unknown".to_owned());
    let empty = empty_ids();
    assert!(!acknowledgement_allowed(
        &batch,
        state(true, false, &empty, &empty, &empty, &empty, &empty),
    ));

    batch.jobs[0].kind = InnerKind::Available;
    batch.message_id = -1;
    let unrequested = BTreeSet::from([23]);
    assert!(!acknowledgement_allowed(
        &batch,
        state(true, false, &empty, &unrequested, &empty, &empty, &empty),
    ));
}

fn state<'a>(
    replay_safe: bool,
    unresolved_available: bool,
    available_requests: &'a BTreeSet<i64>,
    unrequested_requests: &'a BTreeSet<i64>,
    unresolved_requests: &'a BTreeSet<i64>,
    acquired_requests: &'a BTreeSet<i64>,
    completed_requests: &'a BTreeSet<i64>,
) -> AckState<'a> {
    AckState {
        replay_safe,
        unresolved_available,
        available_requests,
        unrequested_requests,
        unresolved_requests,
        acquired_requests,
        completed_requests,
    }
}
