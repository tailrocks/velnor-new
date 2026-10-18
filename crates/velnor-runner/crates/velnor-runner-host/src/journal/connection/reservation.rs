//! Shared capacity accounting and one-use admission.

mod admission;
mod lifecycle;
mod limits;
mod read;
mod storage;

pub(crate) use admission::{ReservationRequest, ReservationResult};
pub(crate) use limits::{CapacityLimits, OperationalCapacityLimits};
pub(crate) use read::{ReservationSnapshot, ReservationState};
