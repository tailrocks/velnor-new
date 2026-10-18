//! Connection-scoped completion records and leases.

mod claim;
mod inbox;
mod proof;
mod record;

pub(crate) use inbox::ConnectionCompletionInboxEntry;
