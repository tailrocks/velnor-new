// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{PairStage, pair_stage};
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
