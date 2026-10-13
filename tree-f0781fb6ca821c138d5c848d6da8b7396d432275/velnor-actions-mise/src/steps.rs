//! Typed `Prepare pinned tools` step requests (task §2, workflow §3).
//!
//! Both contracts name the same bootstrap step: install exact catalog
//! tools through the fixed `mise install` invocation with project
//! config, env files, hooks, and lockfile writes disabled, under
//! Velnor-owned tool homes. This module owns the fixed argv plus the
//! step env; renderers serialize, never invent.

use std::ffi::OsString;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{IsolatedCommand, toolchain_env};
use crate::error::MiseError;
use crate::requests::MiseInstall;

/// Contract-fixed display name shared by task-execution §2 step 1 and
/// workflow §3 step 3. Emitters use this const, never a retyped string.
pub const PREPARE_PINNED_TOOLS_STEP: &str = "Prepare pinned tools";

/// Velnor-owned persistent tool homes carried by every generated step.
///
/// Values are caller-supplied absolute paths or runner-temp expressions;
/// only presence is enforced here, never a shape, so expression forms
/// (`${{ runner.temp }}/...`) pass through untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolHomes {
    /// Velnor-owned `MISE_RUSTUP_HOME` value.
    rustup_home: String,
    /// Velnor-owned `MISE_CARGO_HOME` value.
    cargo_home: String,
}

impl ToolHomes {
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

    /// Exact home env triple: both owned homes plus the exact
    /// `RUSTUP_TOOLCHAIN` pin from the catalog.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        toolchain_env(
            &self.rustup_home,
            &self.cargo_home,
            &catalog.rustup_toolchain(),
        )
    }
}

/// Bootstrap installation of exact catalog tools as one named step.
///
/// Fixed argv from [`MiseInstall`]; the step env adds the owned homes
/// to the isolation quartet. Explicit installation stays enabled: no
/// install-disable pair may appear here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparePinnedTools {
    /// Fixed install request selecting the exact specs.
    install: MiseInstall,
    /// Owned homes carried by the step env.
    homes: ToolHomes,
}

impl PreparePinnedTools {
    /// Prepare `tools` at their exact pins under `homes`.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] for zero tools.
    pub fn new(tools: Vec<PinnedTool>, homes: ToolHomes) -> Result<Self, MiseError> {
        Ok(Self {
            install: MiseInstall::new(tools)?,
            homes,
        })
    }

    /// Contract-fixed display name of the emitted step.
    #[must_use]
    pub fn step_name() -> &'static str {
        PREPARE_PINNED_TOOLS_STEP
    }

    /// Tools installed as `<tool>@<exact>` selectors.
    #[must_use]
    pub fn tools(&self) -> &[PinnedTool] {
        self.install.tools()
    }

    /// Owned homes carried by the step env.
    #[must_use]
    pub fn homes(&self) -> &ToolHomes {
        &self.homes
    }

    /// Full mise argument vector including the program.
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        self.install.argv(catalog)
    }

    /// Full step env: isolation quartet plus the owned-homes triple.
    ///
    /// Matches [`Self::command`]'s spawner env exactly; the
    /// correspondence is pinned by test, not by construction comment.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        env.extend(self.homes.env(catalog));
        env
    }

    /// Isolated command running this installation under the owned homes.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] only if the tool list were
    /// empty, which the constructor rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        Ok(self
            .install
            .command(catalog)?
            .with_env(&self.homes.env(catalog)))
    }
}

/// Reject blank or shell-hostile step tokens (targets, platforms).
///
/// # Errors
///
/// Returns [`MiseError::InvalidStepInput`] for empty values or
/// characters outside ASCII alphanumeric plus `-_.`.
pub(crate) fn validate_step_token(field: &str, value: &str) -> Result<(), MiseError> {
    let clean = !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if clean {
        Ok(())
    } else {
        Err(invalid_input(field, value))
    }
}

/// Build the shared rejection for a malformed step input.
fn invalid_input(field: &str, value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}
