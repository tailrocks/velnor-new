//! Affected-work selection over base/head graphs.
//!
//! Third layer of the orchestrator family: narrows discovery output to
//! the packages and checks a change set affects. Builds on discovery
//! edge primitives; planning and provisioning consume the selection.

pub mod select;
pub mod select_affected;
