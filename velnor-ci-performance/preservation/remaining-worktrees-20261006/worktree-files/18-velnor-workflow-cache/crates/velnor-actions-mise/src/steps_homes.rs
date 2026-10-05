//! Velnor-owned tool home values shared by prepare and verify steps.

use std::ffi::OsString;

use crate::catalog::ToolCatalog;
use crate::command::{IsolatedCommand, NO_AUTO_INSTALL_ENV, toolchain_env};
use crate::error::MiseError;

/// Persistent Mise, Rustup, and Cargo homes carried by generated steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolHomes {
    mise_data_dir: String,
    rustup_home: String,
    cargo_home: String,
}

impl ToolHomes {
    /// Velnor-owned homes under the checked-out workspace.
    #[must_use]
    pub fn workspace_cache() -> Self {
        Self {
            mise_data_dir: crate::runtime_paths::MISE_DATA_DIR_EXPR.to_owned(),
            rustup_home: crate::runtime_paths::RUSTUP_HOME_EXPR.to_owned(),
            cargo_home: crate::runtime_paths::CARGO_HOME_EXPR.to_owned(),
        }
    }

    /// Bind caller-supplied absolute paths or workspace expressions.
    /// # Errors
    pub fn new(
        mise_data_dir: &str,
        rustup_home: &str,
        cargo_home: &str,
    ) -> Result<Self, MiseError> {
        for (field, value) in [
            ("mise_data_dir", mise_data_dir),
            ("rustup_home", rustup_home),
            ("cargo_home", cargo_home),
        ] {
            if value.is_empty() {
                return Err(super::invalid_input(field, value));
            }
        }
        Ok(Self {
            mise_data_dir: mise_data_dir.to_owned(),
            rustup_home: rustup_home.to_owned(),
            cargo_home: cargo_home.to_owned(),
        })
    }

    /// Mise data directory.
    #[must_use]
    pub fn mise_data_dir(&self) -> &str {
        &self.mise_data_dir
    }

    /// Rustup home.
    #[must_use]
    pub fn rustup_home(&self) -> &str {
        &self.rustup_home
    }

    /// Cargo home.
    #[must_use]
    pub fn cargo_home(&self) -> &str {
        &self.cargo_home
    }

    /// Exact Mise, native home, and toolchain environment.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        toolchain_env(
            &self.mise_data_dir,
            &self.rustup_home,
            &self.cargo_home,
            &catalog.rustup_toolchain(),
        )
    }

    /// Isolation, install-disable, and exact owned homes.
    #[must_use]
    pub fn exec_env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        env.extend(
            NO_AUTO_INSTALL_ENV
                .iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value))),
        );
        env.extend(self.env(catalog));
        env
    }
}
