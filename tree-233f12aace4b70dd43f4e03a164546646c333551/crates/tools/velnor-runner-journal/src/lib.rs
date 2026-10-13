//! Durable host substrate: local turso journal, reconcile vocabulary, and the shared host error.
//!
//! Callers persist intent before an external effect and never hold a
//! transaction across that effect. Extracted from velnor-runner-host;
//! `HostError` moves with it because every journal call returns it.

pub mod error;
pub mod journal;
pub mod reconcile;

pub use error::HostError;
pub use journal::{IntentState, Journal, LaunchClaim, Outcome};
pub use reconcile::{
    IntentRow, Reconcile, ReleaseFact, before_advertise, occupies, release_permitted,
};
