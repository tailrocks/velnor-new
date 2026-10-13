//! Exact-generation cleanup proof for one ephemeral runner and private `DinD`.

use crate::HostError;
use zeroize::Zeroizing;

mod diagnostics;
mod docker;
mod journal_ledger;

pub(super) use diagnostics::DiagnosticsReceipt;
pub use diagnostics::{
    DiagnosticsStore, ProtectedStateDirectory, ProtectedStateDirectoryIdentity,
    validate_protected_state_directory,
};
pub use docker::DockerCleanupEngine;

include!("cleanup/types.rs");
include!("cleanup/flow.rs");

#[cfg(test)]
mod tests;
