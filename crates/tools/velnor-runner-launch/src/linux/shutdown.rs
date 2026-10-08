//! Durable drain fencing and exact-generation shutdown reconciliation.

mod identity;
mod reconcile;
mod summary;
mod wait;

pub(super) use reconcile::{cleanup_terminal_workers, reconcile_shutdown};
pub(super) use summary::unresolved_outcome;
pub(super) use wait::{confirm_drain, read_startup_drain, wait_for_shutdown};

#[cfg(test)]
pub(crate) use identity::cleanup_identity;
#[cfg(test)]
pub(crate) use summary::{
    RowCounts, outcome_with_counts, row_counts, summarize_complete_snapshot,
    summarize_snapshot_before_deadline,
};
#[cfg(test)]
pub(crate) use wait::retained_cutoff;
