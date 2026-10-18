//! Shared capacity accounting and one-use admission.

mod admission;
mod enumerate;
mod lifecycle;
mod limits;
mod read;
mod storage;

pub(crate) use admission::{ReservationRequest, ReservationResult};
pub(crate) use enumerate::{
    PhysicalRunnerJitState, PhysicalRunnerOwnership, PhysicalRunnerReservation,
};
pub(crate) use limits::{CapacityLimits, OperationalCapacityLimits};
pub(crate) use read::{ReservationSnapshot, ReservationState};
