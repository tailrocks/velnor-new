//! Generator identity resolution and verification (P03).
//!
//! Declared via `#[path]` from `cover_identity.rs` (no `lib.rs` edit).
//! A generator SHA is verifiable only when it names the running binary:
//! its real SHA-256 hash. All-zero, empty, and unresolved-marker SHAs
//! prove nothing and never validate evidence; no lock fill ever upgrades
//! them, so a source build can never emit a release-pinned identity.

use velnor_actions_orchestrator_graph::internal_plan::snapshot::UNRESOLVED_GENERATOR_SHA;

/// Lookup-skipped reason for an unverifiable source-build generator.
pub(crate) const SOURCE_BUILD_REASON: &str = "generator_unverifiable_source_build";

/// True for generator SHAs that prove nothing: empty, all-zero, or the
/// explicit unresolved marker. No release binary stands behind any of
/// them, so baseline evidence bound to them is unverifiable.
pub(crate) fn is_source_build(sha: &str) -> bool {
    sha.is_empty()
        || sha == UNRESOLVED_GENERATOR_SHA
        || (sha.len() == 64 && sha.bytes().all(|b| b == b'0'))
}

#[cfg(test)]
mod tests;
