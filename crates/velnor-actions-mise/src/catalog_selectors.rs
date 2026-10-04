//! Fallible selectors: native identities require explicit host installation authority.

use super::{PYTHON_VERSION, PinnedTool, RustInstallOptions, ToolCatalog, qualification};
use crate::error::MiseError;

// Exact qualified Aqua registry selector; generic shorthand does not exist.
const NEXTEST_TOOL_SPEC_PREFIX: &str = "aqua:nextest-rs/nextest/cargo-nextest";

impl ToolCatalog {
    /// Exact selector for tools whose identity does not depend on a native host.
    /// # Errors
    /// Rejects native distribution slots without explicit host qualification.
    pub fn tool_spec(&self, tool: PinnedTool) -> Result<String, MiseError> {
        if Self::requires_native_host(tool) {
            return Err(MiseError::Contract {
                problem: format!(
                    "native selector requires explicit host: {}",
                    tool.tool_name()
                ),
            });
        }
        let selector = match tool {
            PinnedTool::Rust => RustInstallOptions::required().pinned_spec(self.version(tool)),
            PinnedTool::RustDesktop => self
                .desktop_compiler_options()
                .pinned_spec(self.version(tool)),
            PinnedTool::Nextest => format!("{NEXTEST_TOOL_SPEC_PREFIX}@{}", self.version(tool)),
            PinnedTool::CargoAudit => format!("cargo:cargo-audit@{}", self.version(tool)),
            PinnedTool::Boltffi => format!("github:boltffi/boltffi@{}", self.version(tool)),
            PinnedTool::SwiftLint => format!("aqua:realm/SwiftLint@{}", self.version(tool)),
            PinnedTool::Periphery => format!("aqua:peripheryapp/periphery@{}", self.version(tool)),
            PinnedTool::Alint => format!("github:asamarts/alint@{}", self.version(tool)),
            PinnedTool::Reuse => format!(
                "pipx:reuse[extras=charset-normalizer,uvx_args=\"--python {PYTHON_VERSION} --no-python-downloads\"]@{}",
                self.version(tool)
            ),
            _ => format!("{}@{}", tool.tool_name(), self.version(tool)),
        };
        Ok(selector)
    }

    /// Exact selector admitted by this host's qualified installation record.
    /// # Errors
    /// Rejects missing provider, artifact, installation or launch qualification.
    pub fn native_tool_spec(
        &self,
        host: qualification::DistributionHost,
        tool: PinnedTool,
    ) -> Result<String, MiseError> {
        if let Some(profile) = super::NativeToolProfile::for_tool(tool) {
            return Ok(profile.selector(host)?.to_owned());
        }
        if Self::requires_native_host(tool) {
            return Ok(self.native_distribution(host, tool)?.selector().to_owned());
        }
        if matches!(tool, PinnedTool::Rust | PinnedTool::RustDesktop)
            && (tool != self.compiler_tool() || host.abi() != self.rust_host().target_triple())
        {
            return Err(MiseError::Contract {
                problem: "explicit host does not match selected compiler role".to_owned(),
            });
        }
        self.tool_spec(tool)
    }

    /// Whether selection requires a host-bound native distribution record.
    #[must_use]
    pub const fn requires_native_host(tool: PinnedTool) -> bool {
        matches!(
            tool,
            PinnedTool::Gh
                | PinnedTool::Bun
                | PinnedTool::Node
                | PinnedTool::Opentofu
                | PinnedTool::Python
                | PinnedTool::Uv
                | PinnedTool::Java
                | PinnedTool::Gradle
                | PinnedTool::CargoSemverChecks
                | PinnedTool::ReleasePlz
        )
    }

    /// Mise selectors for several tools, in the given order.
    /// # Errors
    /// Rejects native slots without an explicit host.
    pub fn tool_specs(&self, tools: &[PinnedTool]) -> Result<Vec<String>, MiseError> {
        tools.iter().map(|tool| self.tool_spec(*tool)).collect()
    }

    /// Exact selectors admitted by the explicit host, preserving input order.
    /// # Errors
    /// Rejects any tool without required native installation qualification.
    pub fn native_tool_specs(
        &self,
        host: qualification::DistributionHost,
        tools: &[PinnedTool],
    ) -> Result<Vec<String>, MiseError> {
        tools
            .iter()
            .map(|tool| self.native_tool_spec(host, *tool))
            .collect()
    }
}
