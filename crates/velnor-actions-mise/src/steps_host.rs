//! Explicit native host binding for tool preparation steps.

use std::ffi::OsString;

use super::PreparePinnedTools;
use crate::catalog::{ToolCatalog, qualification::DistributionHost};
use crate::command::IsolatedCommand;
use crate::error::MiseError;

impl PreparePinnedTools {
    /// Render installation using the actual host's qualified selectors.
    /// # Errors
    /// Rejects missing native qualification or invalid command construction.
    pub fn argv_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
    ) -> Result<Vec<OsString>, MiseError> {
        self.install.argv_for_host(catalog, host)
    }

    /// Run installation under owned homes using the actual native host.
    /// # Errors
    /// Rejects missing native qualification or invalid command construction.
    pub fn command_for_host(
        &self,
        catalog: &ToolCatalog,
        host: DistributionHost,
    ) -> Result<IsolatedCommand, MiseError> {
        self.install
            .command_for_host(catalog, host)?
            .with_env(&self.homes.env(catalog))
    }
}
