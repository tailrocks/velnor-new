//! Cargo metadata discovery and locked/offline qualification requests.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::helpers::full_mise_argv;
use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;

const CARGO_PROGRAM: &str = "cargo";
const CARGO_METADATA: &str = "metadata";

/// Conservative discovery of one manifest through pinned Cargo.
///
/// Exact payload: `cargo metadata --format-version 1 --no-deps
/// --manifest-path <manifest>`. No `--locked`/`--offline`: discovery must not
/// wait for full resolution. `--no-deps` skips resolution entirely, so the
/// probe performs no index access, network fetch, or repository write --
/// not even for lockless-with-dependencies manifests (poison-fixture proven;
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

/// Locked/offline qualification after dependency sources have been prepared.
///
/// Exact payload: `cargo metadata --format-version 1 --locked --offline
/// --all-features --manifest-path <workspace-root>/Cargo.toml`. Missing
/// offline dependencies surface as `preparation_incomplete` upstream, never
/// a fetch. `--all-features` makes the resolve list every optional edge any
/// configuration could activate: affected-work selection walks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataQualification {
    /// Workspace-root manifest whose resolution is qualified.
    workspace_manifest: PathBuf,
}

impl MetadataQualification {
    /// Qualify resolution for one workspace-root manifest.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidManifestPath`] for an empty path.
    pub fn new(workspace_manifest: PathBuf) -> Result<Self, MiseError> {
        if workspace_manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        Ok(Self { workspace_manifest })
    }

    /// Workspace-root manifest whose resolution is qualified.
    #[must_use]
    pub fn workspace_manifest(&self) -> &Path {
        &self.workspace_manifest
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
            OsString::from("--offline"),
            OsString::from("--all-features"),
            OsString::from("--manifest-path"),
            self.workspace_manifest.as_os_str().to_owned(),
        ]
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(catalog, &[PinnedTool::Rust], &self.cargo_argv())
    }

    /// Isolated command running this qualification.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        IsolatedCommand::mise_exec(&specs, &self.cargo_argv())
    }

    /// Run qualification and return the raw full-resolve metadata JSON.
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
