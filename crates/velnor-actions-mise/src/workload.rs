//! Native workload execution through isolated, explicitly pinned Mise.

use std::ffi::OsString;

#[path = "workload_java_env.rs"]
mod java_env;
pub use java_env::java_isolation_prefix;

use crate::{IsolatedCommand, MiseError, PinnedTool, ToolCatalog};

/// One fixed native payload, run in its inspected repository root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadExec {
    root: String,
    tools: Vec<PinnedTool>,
    payload: Vec<OsString>,
}

impl WorkloadExec {
    /// Build a native request; an empty tool set uses runner-provided tools.
    ///
    /// # Errors
    /// Returns `InvalidStepInput` for an unsafe root, or payload validation errors.
    pub fn new(
        root: &str,
        tools: Vec<PinnedTool>,
        payload: Vec<OsString>,
    ) -> Result<Self, MiseError> {
        validate_root(root)?;
        let Some(program) = payload.first() else {
            return Err(MiseError::EmptyCommand {
                program: "mise".to_owned(),
            });
        };
        if program.is_empty() {
            return Err(MiseError::EmptyCommand {
                program: "mise".to_owned(),
            });
        }
        crate::requests::reject_forbidden_payload(program, &payload[1..])?;
        Ok(Self {
            root: root.to_owned(),
            tools,
            payload,
        })
    }

    /// Isolated command; explicit preparation must install all selected tools.
    ///
    /// # Errors
    /// Returns a command validation error if the payload is invalid.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        IsolatedCommand::mise_exec(&catalog.tool_specs(&self.tools)?, &self.payload)?
            .with_mise_root(&self.root)
    }

    /// Isolated native command qualified for the explicit runner host.
    /// # Errors
    /// Rejects absent native installation records or invalid command data.
    pub fn command_for_host(
        &self,
        catalog: &ToolCatalog,
        host: super::qualification::DistributionHost,
    ) -> Result<IsolatedCommand, MiseError> {
        IsolatedCommand::mise_exec(
            &catalog.native_tool_specs(host, &self.tools)?,
            &self.payload,
        )?
        .with_mise_root(&self.root)
    }

    /// Render the workload using the explicit host's admitted selectors.
    /// # Errors
    /// Rejects missing native qualification or invalid command data.
    pub fn argv_for_host(
        &self,
        catalog: &ToolCatalog,
        host: super::qualification::DistributionHost,
    ) -> Result<Vec<OsString>, MiseError> {
        Ok(self.command_for_host(catalog, host)?.argv())
    }

    /// Renderable argv, including `--cd` before `exec`.
    ///
    /// # Errors
    /// Returns a command validation error if the payload is invalid.
    pub fn argv(&self, catalog: &ToolCatalog) -> Result<Vec<OsString>, MiseError> {
        Ok(self.command(catalog)?.argv())
    }
}

/// Enforce relative repository roots before they become global Mise flags.
pub(crate) fn validate_root(root: &str) -> Result<(), MiseError> {
    if root.is_empty()
        || root.starts_with('/')
        || root.starts_with('-')
        || root.contains('\\')
        || root.contains('\0')
        || root.split('/').any(|part| part.is_empty() || part == "..")
    {
        return Err(MiseError::InvalidStepInput {
            field: "workload_root".to_owned(),
            value: root.to_owned(),
        });
    }
    Ok(())
}
