//! Pure append-only model for a branch-restricted pull request transition.
//!
//! The journal owns storage and schema wiring. This model has no API client and cannot perform
//! GitHub effects; callers must commit each returned event before starting the corresponding
//! request.

#[path = "ledger.rs"]
mod ledger;
#[path = "plan.rs"]
mod plan;
#[path = "receipt.rs"]
mod receipt;
#[cfg(test)]
#[path = "tests.rs"]
pub(super) mod tests;
#[path = "types.rs"]
mod types;
#[path = "validation.rs"]
mod validation;

pub(crate) use ledger::{RecoveryAction, TransitionLedger};
pub use plan::{TransitionPlan, TransitionPlanInput};
pub(crate) use receipt::ReferenceReceipt;
pub(crate) use receipt::{NonceCommitReceipt, RetargetHistoryReceipt, RetargetResponseReceipt};
pub(crate) use types::TransitionEvent;
pub use types::{
    ResponseDisposition, TransitionEffect, TransitionError, TransitionStage, UncertaintyReason,
};
