//! Accepted-before-close telemetry lifetime; this is not descendant closure.
mod ledger;
mod observation;
mod provision;
mod sealing;
mod selection;

pub(crate) use ledger::{AdmissionEntry, AdmissionKind, AdmissionOwner, TerminalOutcome};
pub(crate) use provision::ProvisionAttempt;
pub(crate) use sealing::AdmissionClosure;

#[cfg(all(test, unix, feature = "owned-cache-transport"))]
mod tests;
pub(crate) use selection::SelectionAttempt;
