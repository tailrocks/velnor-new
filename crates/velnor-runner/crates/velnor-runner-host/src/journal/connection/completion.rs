//! Connection-scoped completion records and leases.

mod claim;
mod inbox;
mod native_reservation;
mod proof;
mod record;

pub(crate) use inbox::ConnectionCompletionInboxEntry;
pub(crate) use native_reservation::NativeCompletionReservation;
