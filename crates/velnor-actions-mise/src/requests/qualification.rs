//! Locked and offline Cargo metadata qualification commands.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{CARGO_METADATA, CARGO_PROGRAM, MetadataQualification, full_mise_argv, metadata_tools};
use crate::catalog::ToolCatalog;
use crate::command::IsolatedCommand;
use crate::error::MiseError;

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
            OsString::from("--manifest-path"),
            self.workspace_manifest.as_os_str().to_owned(),
        ]
    }

    /// Full mise argument vector including the program.
    /// # Errors
    /// Rejects native selectors without explicit host qualification.
    pub fn argv(&self, catalog: &ToolCatalog) -> Result<Vec<OsString>, MiseError> {
        full_mise_argv(catalog, &metadata_tools(catalog), &self.cargo_argv())
    }

    /// Isolated command running this qualification.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&metadata_tools(catalog))?;
        IsolatedCommand::mise_exec(&specs, &self.cargo_argv())
    }

    /// Run qualification and return the raw metadata JSON string.
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
