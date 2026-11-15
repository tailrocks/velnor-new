// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use super::{
    InventoryEntry, PairStage, pair_stage,
    settling::{Inventory, SETTLING_BOUND, SettlingAges},
};
use crate::{
    config::ConnectionId,
    error::HostError,
    journal::{AssignmentIdentity, AssignmentKey, PhysicalRunnerJitState, PhysicalRunnerOwnership},
};

fn known() -> PhysicalRunnerOwnership {
    let connection = ConnectionId::parse("lab").expect("a valid connection id");
    let identity = AssignmentIdentity::new(
        &connection,
        7,
        AssignmentKey::AssignedRequest { request_id: 3 },
    )
    .expect("a valid assignment");
    PhysicalRunnerOwnership::Known(identity)
}

#[test]
fn a_pair_that_is_serving_a_job_is_audited_as_running() {
    for state in [
        PhysicalRunnerJitState::Ready,
        PhysicalRunnerJitState::Started,
    ] {
        assert_eq!(pair_stage(&known(), state), Ok(PairStage::Running));
    }
}

#[test]
fn a_pair_being_created_or_finished_is_settling_and_not_an_ownership_error() {
    for state in [
        PhysicalRunnerJitState::Absent,
        PhysicalRunnerJitState::Claimed,
        PhysicalRunnerJitState::Bound,
        PhysicalRunnerJitState::StartRequested,
        PhysicalRunnerJitState::Completed,
        PhysicalRunnerJitState::Cleaned,
    ] {
        assert_eq!(pair_stage(&known(), state), Ok(PairStage::Settling));
    }
}

#[test]
fn a_reservation_that_has_no_assignment_yet_is_settling() {
    assert_eq!(
        pair_stage(
            &PhysicalRunnerOwnership::Unknown,
            PhysicalRunnerJitState::Absent
        ),
        Ok(PairStage::Settling)
    );
}

#[test]
fn a_pair_whose_state_is_uncertain_is_still_an_ownership_error() {
    for state in [
        PhysicalRunnerJitState::Uncertain,
        PhysicalRunnerJitState::StartUncertain,
        PhysicalRunnerJitState::Unknown,
    ] {
        assert_eq!(pair_stage(&known(), state), Err(HostError::Ownership));
    }
}

fn entry(reservation_id: i64) -> InventoryEntry {
    InventoryEntry {
        connection_id: "lab".to_owned(),
        reservation_id,
        physical_assignment_key: "key".to_owned(),
        witness_request_id: None,
        acquired_request_key: None,
        runner_request_id: None,
        started_request_id: None,
        completed_request_id: None,
        runner_id: Some("runner".to_owned()),
        dind_id: Some("dind".to_owned()),
    }
}

#[test]
fn an_inventory_of_running_pairs_is_published() {
    let mut inventory = Inventory::default();
    inventory.admit(true, entry(1));
    let digest = inventory.finish(&SettlingAges::default(), Instant::now());
    assert!(matches!(digest, Ok(Some(value)) if value.len() == 64));
}

#[test]
fn a_settling_pair_withholds_the_inventory_without_failing() {
    let mut inventory = Inventory::default();
    inventory.admit(true, entry(1));
    inventory.settle("lab", 2);
    assert_eq!(
        inventory.finish(&SettlingAges::default(), Instant::now()),
        Ok(None)
    );
}

#[test]
fn a_pair_whose_remote_runner_is_gone_is_settling() {
    let mut inventory = Inventory::default();
    inventory.admit(true, entry(1));
    inventory.admit(false, entry(2));
    assert_eq!(
        inventory.finish(&SettlingAges::default(), Instant::now()),
        Ok(None)
    );
}

fn settling_pair(ages: &SettlingAges, now: Instant) -> Result<Option<String>, HostError> {
    let mut inventory = Inventory::default();
    inventory.settle("lab", 2);
    inventory.finish(ages, now)
}

#[test]
fn a_pair_settling_longer_than_the_bound_is_a_failure() {
    let ages = SettlingAges::default();
    let start = Instant::now();
    assert_eq!(settling_pair(&ages, start), Ok(None));
    assert_eq!(settling_pair(&ages, start + SETTLING_BOUND), Ok(None));
    assert_eq!(
        settling_pair(&ages, start + SETTLING_BOUND + Duration::from_secs(1)),
        Err(HostError::SettlingTimeout)
    );
}

#[test]
fn a_pair_that_stops_settling_starts_a_fresh_age_next_time() {
    let ages = SettlingAges::default();
    let start = Instant::now();
    assert_eq!(settling_pair(&ages, start), Ok(None));
    let running = Inventory::default();
    assert!(matches!(
        running.finish(&ages, start + SETTLING_BOUND),
        Ok(Some(_))
    ));
    assert_eq!(
        settling_pair(&ages, start + SETTLING_BOUND + Duration::from_secs(1)),
        Ok(None)
    );
}

#[test]
fn a_running_pair_without_a_known_assignment_is_an_ownership_error() {
    for state in [
        PhysicalRunnerJitState::Ready,
        PhysicalRunnerJitState::Started,
        PhysicalRunnerJitState::Uncertain,
        PhysicalRunnerJitState::StartUncertain,
        PhysicalRunnerJitState::Unknown,
    ] {
        assert_eq!(
            pair_stage(&PhysicalRunnerOwnership::Unknown, state),
            Err(HostError::Ownership)
        );
    }
}
