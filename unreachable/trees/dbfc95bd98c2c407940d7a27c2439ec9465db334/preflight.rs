//! Preflight route proofs: the effective tool route per workspace.
//!
//! For MBX profiles the proof reports the selected Mise tool, its exact
//! version, and the compiler invocation handled by MBX; for Cargo profiles
//! it proves the exact Cargo toolchain with no MBX wrapper. Both carry the
//! cache-format identity, and an unreportable MBX format fails the proof
//! instead of guessing compatibility.

use std::ffi::{OsStr, OsString};

use velnor_actions_contract::cachekey::{FormatInputs, cache_format_id};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use crate::requests::PinnedToolExec;

/// Effective compile route proven for one workspace.
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

    /// Payload program proving this route.
    #[must_use]
    pub const fn program(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    /// Pinned tools selecting this route.
    #[must_use]
    pub fn tools(self) -> Vec<PinnedTool> {
        match self {
            Self::Cargo => vec![PinnedTool::Rust],
            Self::Mbx => vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        }
    }
}

/// Proven effective route: exact specs, probe invocation, format identity.
///
/// The invocation is an identity probe (`<program> --version`) through the
/// route's exact tool selectors, not a build. A lockfile mention alone
/// never proves a route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteProof {
    /// Proven route.
    driver: RouteDriver,
    /// Identity probe through the route's exact selectors.
    probe: PinnedToolExec,
    /// Exact `<tool>@<version>` selectors proving the route.
    specs: Vec<String>,
    /// Cache-format identity over the reported format and generation.
    cache_format_id: String,
}

impl RouteProof {
    /// Proven route.
    #[must_use]
    pub const fn driver(&self) -> RouteDriver {
        self.driver
    }

    /// Exact `<tool>@<version>` selectors proving the route.
    #[must_use]
    pub fn specs(&self) -> &[String] {
        &self.specs
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

/// Prove the effective route for one workspace.
///
/// `format` and `generation` are the adapter-reported compiler/cache format
/// and generation supplied by the caller; empty values fail, never guess.
///
/// # Errors
///
/// Returns [`MiseError::Contract`] when the format is unreportable, and
/// [`MiseError::ForbiddenPayload`] only if the fixed probe were forbidden,
/// which construction rules out.
pub fn prove_route(
    catalog: &ToolCatalog,
    driver: RouteDriver,
    format: &str,
    generation: &str,
) -> Result<RouteProof, MiseError> {
    let inputs = FormatInputs {
        adapter: driver.adapter().to_owned(),
        format: format.to_owned(),
        generation: generation.to_owned(),
    };
    let cache_format_id = cache_format_id(&inputs).map_err(|err| MiseError::Contract {
        problem: err.to_string(),
    })?;
    let tools = driver.tools();
    let specs = catalog.tool_specs(&tools);
    let probe = PinnedToolExec::new(
        tools,
        OsStr::new(driver.program()),
        vec![OsString::from("--version")],
    )?;
    Ok(RouteProof {
        driver,
        probe,
        specs,
        cache_format_id,
    })
}
