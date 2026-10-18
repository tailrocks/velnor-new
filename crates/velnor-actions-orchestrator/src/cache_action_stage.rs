//! Descriptor-bound staging for the one-file Actions cache transport.

mod compression;
mod context;
mod receipt;
mod stage;
mod tar;

#[cfg(test)]
mod tests;

pub use context::StageContext;
pub use receipt::StageReceiptClaim;
pub use stage::{StageReader, StageWriter, create_stage, open_stage, rewrite_stage};
pub use tar::{StageAccess, TarOutcome, run_cache_tar};

/// Gives a stable error code for a rejected stage operation.
pub type StageError = &'static str;
