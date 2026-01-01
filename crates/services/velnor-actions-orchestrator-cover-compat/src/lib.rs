//! Baseline compatibility derivation for coverage lookup and publish.
//!
//! [`cover_compat`] binds the manifest compatibility ID to the
//! execution shape both sides derive from the plan alone, and
//! fingerprints derived artifact names numerically. Lookup, publish,
//! and validation share these derivations without contacting the
//! service.

pub mod cover_compat;
