//! Preflight route selections: the requested tool route per workspace.
//!
//! For MBX profiles, selected identity includes action-owned MBX while the
//! actual Mise probe selects only Rust and resolves `mbx` from the native
//! action's PATH contribution. A route selection does not prove that action
//! ran; workflow composition owns that prerequisite.

use std::ffi::{OsStr, OsString};

use velnor_actions_contract::cachekey::{FormatInputs, cache_format_id};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::requests::PinnedToolExec;
use velnor_actions_mise_core::command::IsolatedCommand;
use velnor_actions_mise_core::error::MiseError;

/// Requested compile route for one workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDriver {
    /// Plain Cargo profile: exact Cargo toolchain, no MBX wrapper.
    Cargo,
    /// MBX profile: compilation handled by the pinned MBX tool.
    Mbx,
}

impl RouteDriver {
    /// Map an execution-profile compile-driver spelling to its route.
    ///
    /// Only the contract spellings resolve; project tool selectors and
    /// unknown drivers resolve to nothing, never to a default route.
    #[must_use]
    pub fn from_compile_driver(value: &str) -> Option<Self> {
        match value {
            "cargo" => Some(Self::Cargo),
            "mbx" => Some(Self::Mbx),
            _ => None,
        }
    }

    /// Adapter family name used in the cache-format identity.
    #[must_use]
    pub const fn adapter(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    /// Payload program selected for this route.
    #[must_use]
    pub const fn program(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    /// Pinned Mise tools selected by the route probe.
    #[must_use]
    pub fn probe_tools(self) -> Vec<PinnedTool> {
        match self {
            Self::Cargo | Self::Mbx => vec![PinnedTool::Rust],
        }
    }

    /// Full selected tool identity, including action-owned MBX where required.
    #[must_use]
    pub fn identity_tools(self) -> Vec<PinnedTool> {
        match self {
            Self::Cargo => vec![PinnedTool::Rust],
            Self::Mbx => vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        }
    }
}

/// Selected route inputs, exact probe invocation, and format identity.
///
/// Identity specs may include tools provided by another workflow owner; the
/// probe selectors contain only tools installed through Mise. The value
/// describes a requested route and never claims that an external action ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSelection {
    /// Selected route.
    driver: RouteDriver,
    /// Identity probe through the route's Mise selectors.
    probe: PinnedToolExec,
    /// Selected exact tool identities, including externally owned tools.
    identity_specs: Vec<String>,
    /// Exact tool selectors actually included in the Mise probe.
    probe_specs: Vec<String>,
    /// Cache-format identity over the reported format and generation.
    cache_format_id: String,
}

impl RouteSelection {
    /// Selected route.
    #[must_use]
    pub const fn driver(&self) -> RouteDriver {
        self.driver
    }

    /// Selected exact tool identities, including external prerequisites.
    #[must_use]
    pub fn identity_specs(&self) -> &[String] {
        &self.identity_specs
    }

    /// Exact tool selectors passed to `mise exec` for this probe.
    #[must_use]
    pub fn probe_specs(&self) -> &[String] {
        &self.probe_specs
    }

    /// Cache-format identity over the reported format and generation.
    #[must_use]
    pub fn cache_format_id(&self) -> &str {
        &self.cache_format_id
    }

    /// Full probe invocation including the program.
    #[must_use]
    pub fn invocation(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        self.probe.argv(catalog)
    }

    /// Isolated command running the probe.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed probe were
    /// empty, which construction rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        self.probe.command(catalog)
    }
}

/// Select the effective route and exact identity probe for one workspace.
///
/// `format` and `generation` are the adapter-reported compiler/cache format
/// and generation supplied by the caller; empty values fail, never guess.
///
/// # Errors
///
/// Returns [`MiseError::Contract`] when the format is unreportable, and
/// [`MiseError::ForbiddenPayload`] only if the fixed probe were forbidden,
/// which construction rules out.
pub fn select_route(
    catalog: &ToolCatalog,
    driver: RouteDriver,
    format: &str,
    generation: &str,
) -> Result<RouteSelection, MiseError> {
    let inputs = FormatInputs {
        adapter: driver.adapter().to_owned(),
        format: format.to_owned(),
        generation: generation.to_owned(),
    };
    let cache_format_id = cache_format_id(&inputs).map_err(|err| MiseError::Contract {
        problem: err.to_string(),
    })?;
    let tools = driver.probe_tools();
    let identity_specs = catalog.tool_specs(&driver.identity_tools());
    let probe_specs = catalog.tool_specs(&tools);
    let probe = PinnedToolExec::new(
        tools,
        OsStr::new(driver.program()),
        vec![OsString::from("--version")],
    )?;
    Ok(RouteSelection {
        driver,
        probe,
        identity_specs,
        probe_specs,
        cache_format_id,
    })
}
