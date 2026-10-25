use std::ffi::{OsStr, OsString};

use velnor_actions_contract::cachekey::{FormatInputs, cache_format_id};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use crate::requests::PinnedToolExec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDriver {
    Cargo,
    Mbx,
}

impl RouteDriver {
    #[must_use]
    pub fn from_compile_driver(value: &str) -> Option<Self> {
        match value {
            "cargo" => Some(Self::Cargo),
            "mbx" => Some(Self::Mbx),
            _ => None,
        }
    }

    #[must_use]
    pub const fn adapter(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    #[must_use]
    pub const fn program(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    #[must_use]
    pub fn probe_tools(self) -> Vec<PinnedTool> {
        match self {
            Self::Cargo | Self::Mbx => vec![PinnedTool::Rust],
        }
    }

    #[must_use]
    pub fn identity_tools(self) -> Vec<PinnedTool> {
        match self {
            Self::Cargo => vec![PinnedTool::Rust],
            Self::Mbx => vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSelection {
    driver: RouteDriver,
    probe: PinnedToolExec,
    identity_specs: Vec<String>,
    probe_specs: Vec<String>,
    cache_format_id: String,
}

impl RouteSelection {
    #[must_use]
    pub const fn driver(&self) -> RouteDriver {
        self.driver
    }

    #[must_use]
    pub fn identity_specs(&self) -> &[String] {
        &self.identity_specs
    }

    #[must_use]
    pub fn probe_specs(&self) -> &[String] {
        &self.probe_specs
    }

    #[must_use]
    pub fn cache_format_id(&self) -> &str {
        &self.cache_format_id
    }

    #[must_use]
    pub fn invocation(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        self.probe.argv(catalog)
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        self.probe.command(catalog)
    }
}

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
