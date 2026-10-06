//! Isolated native domain adapters; each owns typed policy and fixed sources.
//!
//! Domain modules depend only on the common contract. Orchestration composes
//! their proposals; Mise owns installation and launch; renderer owns YAML.

pub mod apt;
pub mod homebrew;
pub mod java;
pub mod node;
pub mod oci;
pub mod reuse;
pub mod ruby;
pub mod shell;
mod support;
pub mod swift;

pub use support::{OwnedSupportFile, SupportBundle};
