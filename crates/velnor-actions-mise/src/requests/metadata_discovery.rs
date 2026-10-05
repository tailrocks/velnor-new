//! Conservative metadata discovery through pinned Cargo.
//!
//! Discovery requires `--locked` and never resolves (`--no-deps`: no fetch, no
//! lockfile write). Full resolution is qualification's job, lockful-only.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{CARGO_METADATA, CARGO_PROGRAM, full_mise_argv};
use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;

/// Conservative discovery of one manifest through pinned Cargo.
///
/// Exact payload: `cargo metadata --format-version 1 --locked --no-deps
/// --manifest-path <manifest>`. Discovery must not wait for full resolution.
/// `--locked` enforces lock immutability without
/// requiring a lockfile when `--no-deps` skips dependency resolution. The
/// probe performs no index access, network fetch, or repository write --
/// even for lockless-with-dependencies manifests (poison-fixture proven;
/// the orchestrator also brackets every run with a tool snapshot that fails
/// closed on drift). Full resolution is qualification's job, lockful-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataDiscovery {
    /// Manifest whose metadata is requested.
    manifest: PathBuf,
}

impl MetadataDiscovery {
    /// Discover metadata for one manifest path.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidManifestPath`] for an empty path.
    pub fn new(manifest: PathBuf) -> Result<Self, MiseError> {
        if manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        Ok(Self { manifest })
    }

    /// Manifest whose metadata is requested.
    #[must_use]
    pub fn manifest(&self) -> &Path {
        &self.manifest
    }

    /// Cargo-side payload arguments, byte-exact per the contract.
    #[must_use]
    pub fn cargo_argv(&self) -> Vec<OsString> {
        vec![
            OsString::from(CARGO_PROGRAM),
            OsString::from(CARGO_METADATA),
            OsString::from("--format-version"),
            OsString::from("1"),
            OsString::from("--locked"),
            OsString::from("--no-deps"),
            OsString::from("--manifest-path"),
            self.manifest.as_os_str().to_owned(),
        ]
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(catalog, &[PinnedTool::Rust], &self.cargo_argv())
    }

    /// Isolated command running this discovery.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        IsolatedCommand::mise_exec(&specs, &self.cargo_argv())
    }

    /// Run discovery and return the raw metadata JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when Cargo cannot launch,
    /// [`MiseError::NonZeroExit`] on nonzero status, and
    /// [`MiseError::InvalidUtf8`] when stdout is not text.
    pub fn run(&self, catalog: &ToolCatalog) -> Result<String, MiseError> {
        let output = self.command(catalog)?.run()?;
        output.require_success("mise")?;
        output.stdout_text("mise")
    }
}
