// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gc1_t22_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_ceiling_policy()?;
    check_inventory_accounting()?;
    let _admitted = check_admission_round_trip()?;
    check_corrupt_stays_charged()?;
    Ok(())
}

fn check_unknowns_stay_unknown() -> Result<(), Box<dyn Error>> {
    let unknown = mark_inventory_unknown(1, "2026-10-25T02:00:00Z", 0, "pagination blocked")?;
    if unknown.complete
        || unknown.sum_stored_bytes.value.is_some()
        || unknown.unknown_reason.is_none()
    {
        return Err(fail("unknown inventory claimed a measured sum"));
    }
    let bytes = inventory_canonical_bytes(&unknown)?;
    if parse_inventory_bytes(&bytes)? != unknown {
        return Err(fail("unknown inventory strict round trip drifted"));
    }
    let mut request = admission_request(4_200, 1_000_000)?;
    request.inventory_bytes = unknown_bytes("pagination blocked")?;
    let blocked = decide_admission(&request)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_INVENTORY.to_owned())
    {
        return Err(fail("unknown inventory missed its block"));
    }
    let mut foreign = admission_request(4_200, 1_000_000)?;
    foreign.foreign_inflight_bytes = unknown_bytes("foreign writers unbounded")?;
    let blocked = decide_admission(&foreign)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_FOREIGN_EXPOSURE.to_owned())
    {
        return Err(fail("unknown foreign exposure missed its block"));
    }
    let mut envelope = admission_request(4_200, 1_000_000)?;
    envelope.envelope = OuterEnvelope::Unknown(unknown_envelope(
        UNKNOWN_TOOL_IDENTITY,
        "pinned tool behavior not yet qualified",
    )?);
    let blocked = decide_admission(&envelope)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_RUNTIME_ENVELOPE.to_owned())
    {
        return Err(fail("unknown runtime envelope missed its block"));
    }
    let mut disk = admission_request(4_200, 1_000_000)?;
    disk.available_local_disk = unknown_bytes("runner disk not measured")?;
    let blocked = decide_admission(&disk)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_UNKNOWN_LOCAL_DISK.to_owned())
    {
        return Err(fail("unknown local disk missed its block"));
    }
    Ok(())
}

fn check_headroom_and_slots() -> Result<(), Box<dyn Error>> {
    let tight = decide_admission(&admission_request(4_200, OPERATING_CEILING_BYTES)?)?;
    if tight.decision != velnor_actions_orchestrator::AdmissionDecision::Blocked
        || !tight
            .blocked_reasons
            .contains(&BLOCKED_INSUFFICIENT_HEADROOM.to_owned())
    {
        return Err(fail("over-ceiling candidate missed its headroom block"));
    }
    let mut deletion = admission_request(4_200, 1_000_000)?;
    deletion.claimed_freed_bytes = 2_500;
    must_fail!(decide_admission(&deletion), "unconfirmed deletion credit");
    let mut full = admission_request(4_200, 1_000_000)?;
    full.occupied_generation_slots = 2;
    let blocked = decide_admission(&full)?;
    if !blocked
        .blocked_reasons
        .contains(&BLOCKED_GENERATION_SLOTS_EXHAUSTED.to_owned())
    {
        return Err(fail("full generation slots missed their block"));
    }
    let mut beyond = admission_request(4_200, 1_000_000)?;
    beyond.occupied_generation_slots = 3;
    must_fail!(decide_admission(&beyond), "occupied beyond maximum");
    let mut reserve = admission_request(4_200, 1_000_000)?;
    reserve.operating_ceiling_bytes = HARD_CEILING_BYTES;
    must_fail!(decide_admission(&reserve), "reserve below minimum");
    let mut tight_disk = admission_request(4_200, 1_000_000)?;
    tight_disk.available_local_disk = measured_bytes(1_000_000);
    let blocked = decide_admission(&tight_disk)?;
    if !blocked
        .blocked_reasons
        .contains(&velnor_actions_orchestrator::BLOCKED_INSUFFICIENT_LOCAL_DISK.to_owned())
    {
        return Err(fail("tight local disk missed its block"));
    }
    Ok(())
}

fn check_rejects_invalid_inventory() -> Result<(), Box<dyn Error>> {
    must_fail!(
        build_inventory(0, "2026-10-25T02:00:00Z", Vec::new(), 1),
        "zero repository",
    );
    must_fail!(
        build_inventory(
            1,
            "2026-10-25T02:00:00Z",
            vec![entry(0, "velnor-gc1-state-x", "refs/heads/main", 8)],
            1,
        ),
        "zero cache id",
    );
    must_fail!(
        build_inventory(1, "2026-10-25T02:00:00Z", Vec::new(), 0),
        "complete without pages",
    );
    let mut mismatch_repo = entry(31, "velnor-gc1-state-x", "refs/heads/main", 8);
    mismatch_repo.repository_id = 2;
    must_fail!(
        build_inventory(1, "2026-10-25T02:00:00Z", vec![mismatch_repo], 1),
        "entry repository mismatch",
    );
    let complete = build_inventory(1, "2026-10-25T02:00:00Z", vec![entry(41, "k", "r", 8)], 1)?;
    let mut duplicate = complete.clone();
    duplicate
        .entries
        .push(entry(41, "velnor-gc1-state-y", "refs/heads/main", 9));
    must_fail!(inventory_canonical_bytes(&duplicate), "duplicate cache id",);
    let mut drifted_sum = complete.clone();
    drifted_sum.sum_stored_bytes = measured_bytes(9);
    must_fail!(inventory_canonical_bytes(&drifted_sum), "sum mismatch");
    let mut bad_schema = complete.clone();
    bad_schema.schema = 2;
    must_fail!(inventory_canonical_bytes(&bad_schema), "inventory schema");
    let mut without_reason = mark_inventory_unknown(1, "t", 0, "detail")?;
    without_reason.unknown_reason = None;
    must_fail!(
        inventory_canonical_bytes(&without_reason),
        "incomplete without reason",
    );
    let bytes = inventory_canonical_bytes(&complete)?;
    must_fail!(
        parse_inventory_bytes(&bytes[..bytes.len() / 2]),
        "truncated inventory",
    );
    must_fail!(
        parse_inventory_bytes(br#"{"schema":1,"schema":1}"#),
        "duplicate key",
    );
    let mut value = serde_json::to_value(&complete)?;
    value
        .as_object_mut()
        .ok_or_else(|| fail("inventory not object"))?
        .insert("unexpected".to_owned(), serde_json::Value::from(1));
    must_fail!(
        parse_inventory_bytes(&serde_json::to_vec(&value)?),
        "unknown field",
    );
    Ok(())
}

fn check_rejects_invalid_admission(admitted: &AdmissionV1) -> Result<(), Box<dyn Error>> {
    let mut drifted_ceiling = admitted.clone();
    drifted_ceiling.hard_ceiling_bytes = 10 * 1024 * 1024 * 1024;
    must_fail!(
        admission_canonical_bytes(&drifted_ceiling),
        "hard ceiling drift",
    );
    let mut drifted_slots = admitted.clone();
    drifted_slots.maximum_generation_slots = 3;
    must_fail!(
        admission_canonical_bytes(&drifted_slots),
        "generation slots drift",
    );
    let mut with_reasons = admitted.clone();
    with_reasons
        .blocked_reasons
        .push(BLOCKED_INSUFFICIENT_HEADROOM.to_owned());
    must_fail!(
        admission_canonical_bytes(&with_reasons),
        "admitted with reasons",
    );
    let mut bad_reason = admitted.clone();
    bad_reason.decision = velnor_actions_orchestrator::AdmissionDecision::Blocked;
    bad_reason.blocked_reasons = vec!["made-up-reason".to_owned()];
    must_fail!(admission_canonical_bytes(&bad_reason), "unknown reason");
    let mut unsorted = admitted.clone();
    unsorted.decision = velnor_actions_orchestrator::AdmissionDecision::Blocked;
    unsorted.blocked_reasons = vec![
        BLOCKED_UNKNOWN_INVENTORY.to_owned(),
        BLOCKED_INSUFFICIENT_HEADROOM.to_owned(),
    ];
    if unsorted
        .blocked_reasons
        .windows(2)
        .all(|pair| pair[0] < pair[1])
    {
        return Err(fail("reason fixture sorted by accident"));
    }
    must_fail!(admission_canonical_bytes(&unsorted), "unsorted reasons");
    let mut no_reasons = admitted.clone();
    no_reasons.decision = velnor_actions_orchestrator::AdmissionDecision::Blocked;
    must_fail!(
        admission_canonical_bytes(&no_reasons),
        "blocked without reasons",
    );
    let bytes = admission_canonical_bytes(admitted)?;
    must_fail!(
        parse_admission_bytes(&bytes[..bytes.len() / 2]),
        "truncated admission",
    );
    let mut value = serde_json::to_value(admitted)?;
    value
        .as_object_mut()
        .ok_or_else(|| fail("admission not object"))?
        .insert("unexpected".to_owned(), serde_json::Value::from(1));
    must_fail!(
        parse_admission_bytes(&serde_json::to_vec(&value)?),
        "unknown field",
    );
    Ok(())
}

#[test]
fn gc1_t22_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_unknowns_stay_unknown()?;
    check_headroom_and_slots()?;
    check_rejects_invalid_inventory()?;
    let admitted = decide_admission(&admission_request(4_200, 1_000_000)?)?;
    check_rejects_invalid_admission(&admitted)?;
    Ok(())
}
