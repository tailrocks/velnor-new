//! Runtime-domain bindings for pinned installs and executions.

use crate::catalog::ToolCatalog;
use crate::command::{IsolatedCommand, ProcessOutput};
use crate::error::MiseError;
use crate::requests::{MiseInstall, PinnedToolExec};
use crate::runtime_paths::RuntimePaths;

impl PinnedToolExec {
    /// Bind this execution to a compiled generator runtime domain.
    ///
    /// The ordinary command keeps its inherited root. Planning callers opt
    /// into the separate root through this typed method; no user path enters.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the payload were empty,
    /// which the constructor rules out.
    pub fn command_with_runtime(
        &self,
        catalog: &ToolCatalog,
        runtime: RuntimePaths,
    ) -> Result<IsolatedCommand, MiseError> {
        Ok(self.command(catalog)?.with_runtime_paths(runtime))
    }

    /// Run this payload under a compiled generator runtime domain.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when the child cannot be spawned
    /// or reaped. A nonzero exit is returned as data.
    pub fn run_with_runtime(
        &self,
        catalog: &ToolCatalog,
        runtime: RuntimePaths,
    ) -> Result<ProcessOutput, MiseError> {
        self.command_with_runtime(catalog, runtime)?.run()
    }
}

impl MiseInstall {
    /// Bind this install to a compiled generator runtime domain.
    ///
    /// Planning uses this route to install `gh` and validators into the
    /// dedicated root. Full task installation keeps the ordinary command.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] only if the tool list were empty,
    /// which the constructor rules out.
    pub fn command_with_runtime(
        &self,
        catalog: &ToolCatalog,
        runtime: RuntimePaths,
    ) -> Result<IsolatedCommand, MiseError> {
        Ok(self.command(catalog)?.with_runtime_paths(runtime))
    }

    /// Run this install under a compiled generator runtime domain.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when the child cannot be spawned
    /// or reaped. A nonzero exit is returned as data.
    pub fn run_with_runtime(
        &self,
        catalog: &ToolCatalog,
        runtime: RuntimePaths,
    ) -> Result<ProcessOutput, MiseError> {
        self.command_with_runtime(catalog, runtime)?.run()
    }
}
