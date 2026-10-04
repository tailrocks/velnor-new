//! Stored tool homes and the shared domain environment binding.

use std::ffi::OsString;

use super::{ToolHomes, invalid_input};
use crate::{
    MiseError, ToolCatalog,
    command::{IsolatedCommand, NO_AUTO_INSTALL_ENV, toolchain_env},
};

impl ToolHomes {
    /// Velnor-owned tool homes under runner temp (expression form).
    ///
    /// Shell `$VAR` never expands in the `env:` position that carries
    /// these paths; the `${{ runner.temp }}` expression form does.
    #[must_use]
    pub fn runner_temp() -> Self {
        Self {
            rustup_home: crate::runtime_paths::RUSTUP_HOME_EXPR.to_owned(),
            cargo_home: crate::runtime_paths::CARGO_HOME_EXPR.to_owned(),
        }
    }

    /// Bind the two owned home values.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] for an empty home.
    pub fn new(rustup_home: &str, cargo_home: &str) -> Result<Self, MiseError> {
        if rustup_home.is_empty() {
            return Err(invalid_input("rustup_home", rustup_home));
        }
        if cargo_home.is_empty() {
            return Err(invalid_input("cargo_home", cargo_home));
        }
        Ok(Self {
            rustup_home: rustup_home.to_owned(),
            cargo_home: cargo_home.to_owned(),
        })
    }

    /// Velnor-owned `MISE_RUSTUP_HOME` value.
    #[must_use]
    pub fn rustup_home(&self) -> &str {
        &self.rustup_home
    }

    /// Velnor-owned `MISE_CARGO_HOME` value.
    #[must_use]
    pub fn cargo_home(&self) -> &str {
        &self.cargo_home
    }

    /// The four home bindings describe the complete installed payload location.
    /// No compiler selector, toolchain override or install authority is added.
    #[must_use]
    pub fn home_env(&self) -> Vec<(OsString, OsString)> {
        velnor_actions_contract::workflow::tool_producer::homes::bind(
            &self.rustup_home,
            &self.cargo_home,
        )
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
    }

    /// Bind Full homes independently of requested tools; isolated domains get none.
    #[must_use]
    pub fn domain_home_env(
        domain: velnor_actions_contract::ToolCacheDomain,
    ) -> Vec<(OsString, OsString)> {
        domain
            .home_environment()
            .into_iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect()
    }

    /// Four home bindings, the data root and exact compiler pin.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        toolchain_env(
            &self.rustup_home,
            &self.cargo_home,
            &catalog.rustup_toolchain(),
        )
    }

    /// Full env for a pinned `exec` verification step.
    ///
    /// Isolation overlay, install-disable pair and compiler environment:
    /// the step runs the prepared toolchain, and a missing tool
    /// fails as a preparation error instead of installing.
    #[must_use]
    pub fn exec_env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        for (key, value) in NO_AUTO_INSTALL_ENV {
            env.push((OsString::from(key), OsString::from(value)));
        }
        env.extend(self.env(catalog));
        env
    }
}
