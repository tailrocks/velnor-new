use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::helpers::full_mise_argv;
use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;

const CARGO_PROGRAM: &str = "cargo";
const CARGO_METADATA: &str = "metadata";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataDiscovery {
    manifest: PathBuf,
}

impl MetadataDiscovery {
    pub fn new(manifest: PathBuf) -> Result<Self, MiseError> {
        if manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        Ok(Self { manifest })
    }

    #[must_use]
    pub fn manifest(&self) -> &Path {
        &self.manifest
    }

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

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(catalog, &[PinnedTool::Rust], &self.cargo_argv())
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        IsolatedCommand::mise_exec(&specs, &self.cargo_argv())
    }

    pub fn run(&self, catalog: &ToolCatalog) -> Result<String, MiseError> {
        let output = self.command(catalog)?.run()?;
        output.require_success("mise")?;
        output.stdout_text("mise")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataQualification {
    workspace_manifest: PathBuf,
}

impl MetadataQualification {
    pub fn new(workspace_manifest: PathBuf) -> Result<Self, MiseError> {
        if workspace_manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        Ok(Self { workspace_manifest })
    }

    #[must_use]
    pub fn workspace_manifest(&self) -> &Path {
        &self.workspace_manifest
    }

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

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        full_mise_argv(catalog, &[PinnedTool::Rust], &self.cargo_argv())
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        IsolatedCommand::mise_exec(&specs, &self.cargo_argv())
    }

    pub fn run(&self, catalog: &ToolCatalog) -> Result<String, MiseError> {
        let output = self.command(catalog)?.run()?;
        output.require_success("mise")?;
        output.stdout_text("mise")
    }
}
