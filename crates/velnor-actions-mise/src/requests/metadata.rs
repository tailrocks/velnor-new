//! Read-only metadata discovery and locked resolution qualification requests.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::error::MiseError;

use super::{MetadataCommand, full_mise_argv, metadata_mbx_argv};

/// Conservative discovery of one manifest through pinned MBX.
///
/// Exact payload: `mbx +<rust> metadata --format-version 1 --no-deps
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

    /// MBX payload arguments, byte-exact per the contract.
    #[must_use]
    pub fn mbx_argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        metadata_mbx_argv(catalog, &self.manifest, &["--no-deps"])
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(catalog, &[PinnedTool::Rust], &self.mbx_argv(catalog))
    }

    /// Guarded command running this discovery.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<MetadataCommand, MiseError> {
        MetadataCommand::new(catalog, self.mbx_argv(catalog))
    }

    /// Verify action-owned MBX, run discovery, and return raw metadata JSON.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidToolVersion`] when action-owned MBX does
    /// not match the catalog pin, [`MiseError::SpawnFailed`] when a process
    /// cannot launch, [`MiseError::NonZeroExit`] on nonzero metadata status,
    /// and [`MiseError::InvalidUtf8`] when stdout is not text.
    pub fn run(&self, catalog: &ToolCatalog) -> Result<String, MiseError> {
        let output = self.command(catalog)?.run()?;
        output.require_success("mise")?;
        output.stdout_text("mise")
    }
}

/// Locked/offline qualification after dependency sources have been prepared.
///
/// Exact payload: `mbx +<rust> metadata --format-version 1 --locked --offline
/// --manifest-path <workspace-root>/Cargo.toml`. Missing offline
/// dependencies surface as `preparation_incomplete` upstream, never a fetch.
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

    /// MBX payload arguments, byte-exact per the contract.
    #[must_use]
    pub fn mbx_argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        metadata_mbx_argv(
            catalog,
            &self.workspace_manifest,
            &["--locked", "--offline"],
        )
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(catalog, &[PinnedTool::Rust], &self.mbx_argv(catalog))
    }

    /// Guarded command running this qualification.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<MetadataCommand, MiseError> {
        MetadataCommand::new(catalog, self.mbx_argv(catalog))
    }

    /// Verify action-owned MBX, run qualification, and return raw metadata JSON.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidToolVersion`] when action-owned MBX does
    /// not match the catalog pin, [`MiseError::SpawnFailed`] when a process
    /// cannot launch, [`MiseError::NonZeroExit`] on nonzero metadata status,
    /// and [`MiseError::InvalidUtf8`] when stdout is not text.
    pub fn run(&self, catalog: &ToolCatalog) -> Result<String, MiseError> {
        let output = self.command(catalog)?.run()?;
        output.require_success("mise")?;
        output.stdout_text("mise")
    }
}
