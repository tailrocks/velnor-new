//! Explicit host bindings for native installation and execution selectors.

use std::ffi::OsString;

use super::{MiseInstall, PinnedToolExec};
use crate::catalog::{ToolCatalog, qualification::DistributionHost};
use crate::command::{EnvPolicy, IsolatedCommand, ProcessOutput};
use crate::error::MiseError;
use crate::runtime_paths::RuntimePaths;

impl MiseInstall {
    /// Build an isolated command using the explicit host's installation records.
    /// # Errors
    /// Rejects native slots without qualified artifact and installation plans.
    pub fn command_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
    ) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.native_tool_specs(host, &self.tools)?;
        let command = IsolatedCommand::mise_install(&specs)?;
        Ok(command)
    }

    /// Render this request using only explicitly qualified host selectors.
    /// # Errors
    /// Rejects missing native qualification or an invalid command.
    pub fn argv_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
    ) -> Result<Vec<OsString>, MiseError> {
        Ok(self.command_for_host(catalog, host)?.argv())
    }

    /// Run the request using the explicit distribution host.
    /// # Errors
    /// Returns qualification, command construction or process failures.
    pub fn run_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
    ) -> Result<ProcessOutput, MiseError> {
        self.command_for_host(catalog, host)?.run()
    }

    /// Bind qualified host selection to the compiled runtime root.
    /// # Errors
    /// Rejects missing native qualification or an invalid command.
    pub fn command_with_runtime_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
        runtime: RuntimePaths,
    ) -> Result<IsolatedCommand, MiseError> {
        Ok(self
            .command_for_host(catalog, host)?
            .with_runtime_paths(runtime))
    }

    /// Run qualified native selection under a compiled runtime root.
    /// # Errors
    /// Returns qualification, command construction or process failures.
    pub fn run_with_runtime_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
        runtime: RuntimePaths,
    ) -> Result<ProcessOutput, MiseError> {
        self.command_with_runtime_for_host(catalog, host, runtime)?
            .run()
    }
}

impl PinnedToolExec {
    /// Build an isolated command using the explicit host's installation records.
    /// # Errors
    /// Rejects native slots without qualified artifact and installation plans.
    pub fn command_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
    ) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.native_tool_specs(host, &self.tools)?;
        let command = IsolatedCommand::mise_exec(&specs, &self.payload())?;
        if self.tools.as_slice() == [crate::catalog::PinnedTool::Gh] {
            return Ok(command.with_policy(EnvPolicy::Baseline));
        }
        Ok(command)
    }

    /// Render this request using only explicitly qualified host selectors.
    /// # Errors
    /// Rejects missing native qualification or an invalid command.
    pub fn argv_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
    ) -> Result<Vec<OsString>, MiseError> {
        Ok(self.command_for_host(catalog, host)?.argv())
    }

    /// Run the request using the explicit distribution host.
    /// # Errors
    /// Returns qualification, command construction or process failures.
    pub fn run_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
    ) -> Result<ProcessOutput, MiseError> {
        self.command_for_host(catalog, host)?.run()
    }

    /// Bind qualified host selection to the compiled runtime root.
    /// # Errors
    /// Rejects missing native qualification or an invalid command.
    pub fn command_with_runtime_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
        runtime: RuntimePaths,
    ) -> Result<IsolatedCommand, MiseError> {
        Ok(self
            .command_for_host(catalog, host)?
            .with_runtime_paths(runtime))
    }

    /// Run qualified native selection under a compiled runtime root.
    /// # Errors
    /// Returns qualification, command construction or process failures.
    pub fn run_with_runtime_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
        runtime: RuntimePaths,
    ) -> Result<ProcessOutput, MiseError> {
        self.command_with_runtime_for_host(catalog, host, runtime)?
            .run()
    }
}
