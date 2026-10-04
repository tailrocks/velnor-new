//! Typed `Prepare pinned tools` step requests (task §2, workflow §3).
//!
//! Exact catalog installation and fixed step argv under owned tool homes.
//! Project config, env files, hooks and lockfile writes stay disabled.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{IsolatedCommand, mise_argv_tail};
use crate::error::MiseError;
use crate::requests::{MetadataQualification, MiseInstall};

#[path = "steps_host.rs"]
mod host;

#[path = "steps_homes.rs"]
mod homes;

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

/// Bootstrap installation of exact catalog tools as one named step.
///
/// Fixed argv from [`MiseInstall`]; the step env adds the owned homes
/// to the full isolation overlay (config, env files, hooks, and
/// lockfile writes all disabled). Explicit installation stays enabled:
/// no install-disable pair may appear here.
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
    /// # Errors
    /// Returns an error when the selected tool lacks catalog authority.
    pub fn argv(&self, catalog: &ToolCatalog) -> Result<Vec<OsString>, MiseError> {
        self.install.argv(catalog)
    }

    /// Full step env: isolation overlay, four homes, data root and compiler pin.
    ///
    /// Matches [`Self::command`]'s spawner env exactly; the
    /// correspondence is pinned by test, not by construction comment.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        env.extend(self.homes.env(catalog));
        env
    }

    /// Step environment from the selected data domain, without compiler authority.
    #[must_use]
    pub fn env_for_domain(
        domain: velnor_actions_contract::ToolCacheDomain,
    ) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        env.extend(ToolHomes::domain_home_env(domain));
        env
    }

    /// Isolated command running this installation under the owned homes.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] only if the tool list were
    /// empty, which the constructor rules out. Catalog selection errors propagate.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        self.install
            .command(catalog)?
            .with_env(&self.homes.env(catalog))
    }
}

/// Contract-fixed display name of the Rust-components prepare step.
/// Emitters use this const, never a retyped string.
pub const PREPARE_RUST_COMPONENTS_STEP: &str = "Prepare Rust components";

/// Rust components the prepare step guarantees, sorted.
const RUST_COMPONENTS: [&str; 2] = ["clippy", "rustfmt"];

/// Sanctioned pinned-toolchain rustup invocation under owned homes.
const FORBIDDEN_ACKNOWLEDGED_RUSTUP: &str = "rustup";

/// Fixed `rustup component add` for the pinned toolchain as one named step.
///
/// Mise installs the pinned Rust toolchain with typed minimal-profile
/// options including clippy/rustfmt. This idempotent owner operation
/// also verifies/fills their availability after restoration. This is the pinned
/// toolchain's own rustup running one fixed deterministic argv that writes
/// only the Velnor-owned `RUSTUP_HOME` during the online prepare phase:
/// Mise installing components, not an ad hoc installer. The explicit
/// `--toolchain <exact>-<triple>` cannot resolve an ambient toolchain,
/// and implicit installation stays disabled, so a toolchain missing from
/// the owned homes fails as a preparation error instead of installing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRustComponents {
    /// Owned homes carried by the step env.
    homes: ToolHomes,
}

impl PrepareRustComponents {
    /// Guarantee the fixed components under `homes`.
    #[must_use]
    pub fn new(homes: ToolHomes) -> Self {
        Self { homes }
    }

    /// Contract-fixed display name of the emitted step.
    #[must_use]
    pub fn step_name() -> &'static str {
        PREPARE_RUST_COMPONENTS_STEP
    }

    /// Fixed components installed, sorted.
    #[must_use]
    pub fn components() -> Vec<String> {
        RUST_COMPONENTS.iter().map(ToString::to_string).collect()
    }

    /// Owned homes carried by the step env.
    #[must_use]
    pub fn homes(&self) -> &ToolHomes {
        &self.homes
    }

    /// Full mise argument vector including the program.
    /// # Errors
    /// Returns an error when the selected tool lacks catalog authority.
    pub fn argv(&self, catalog: &ToolCatalog) -> Result<Vec<OsString>, MiseError> {
        let specs = Self::tool_specs(catalog)?;
        let mut argv = vec![OsString::from("mise")];
        argv.extend(mise_argv_tail("exec", &specs, &Self::payload(catalog)));
        Ok(argv)
    }

    /// Full step env: isolation plus install-disable plus owned homes.
    ///
    /// Matches [`Self::command`]'s spawner env exactly; the
    /// correspondence is pinned by test, not by construction comment.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        self.homes.exec_env(catalog)
    }

    /// Isolated command running this installation under the owned homes.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which construction rules out. Catalog selection errors propagate.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = Self::tool_specs(catalog)?;
        IsolatedCommand::mise_exec(&specs, &Self::payload(catalog))?
            .with_env(&self.homes.env(catalog))
    }

    /// Fixed payload: `rustup component add --toolchain <name> clippy rustfmt`.
    fn payload(catalog: &ToolCatalog) -> Vec<OsString> {
        let mut payload = vec![
            OsString::from(FORBIDDEN_ACKNOWLEDGED_RUSTUP),
            OsString::from("component"),
            OsString::from("add"),
            OsString::from("--toolchain"),
            OsString::from(catalog.rust_toolchain_name()),
        ];
        payload.extend(RUST_COMPONENTS.iter().map(OsString::from));
        payload
    }

    fn tool_specs(catalog: &ToolCatalog) -> Result<Vec<String>, MiseError> {
        let mut tools = vec![catalog.compiler_tool()];
        if catalog.rust_uses_mbx() {
            tools.push(PinnedTool::MrBoxington);
        }
        catalog.tool_specs(&tools)
    }
}

/// Contract-fixed display name of the task-execution §2 prepared-inputs
/// step. Emitters use this const, never a retyped string.
pub const VERIFY_PREPARED_INPUTS_STEP: &str = "Verify prepared inputs";

/// Locked/offline preparation qualification as one named step.
///
/// Fixed argv from [`MetadataQualification`]; the step env adds the
/// install disable plus the owned homes to the isolation quartet, so a
/// missing tool or dependency fails instead of fetching. Explicit
/// installation stays out: this step verifies, never installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyPreparedInputs {
    /// Fixed locked/offline qualification request.
    qualification: MetadataQualification,
    /// Owned homes carried by the step env.
    homes: ToolHomes,
}

impl VerifyPreparedInputs {
    /// Qualify `workspace_manifest` under `homes`.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidManifestPath`] for an empty manifest.
    pub fn new(workspace_manifest: PathBuf, homes: ToolHomes) -> Result<Self, MiseError> {
        Ok(Self {
            qualification: MetadataQualification::new(workspace_manifest)?,
            homes,
        })
    }

    /// Contract-fixed display name of the emitted step.
    #[must_use]
    pub fn step_name() -> &'static str {
        VERIFY_PREPARED_INPUTS_STEP
    }

    /// Workspace-root manifest whose resolution is qualified.
    #[must_use]
    pub fn workspace_manifest(&self) -> &std::path::Path {
        self.qualification.workspace_manifest()
    }

    /// Owned homes carried by the step env.
    #[must_use]
    pub fn homes(&self) -> &ToolHomes {
        &self.homes
    }

    /// Full mise argument vector including the program.
    /// # Errors
    /// Returns an error when the selected tool lacks catalog authority.
    pub fn argv(&self, catalog: &ToolCatalog) -> Result<Vec<OsString>, MiseError> {
        self.qualification.argv(catalog)
    }

    /// Full step env: isolation plus install-disable plus owned homes.
    ///
    /// Matches [`Self::command`]'s spawner env exactly; the
    /// correspondence is pinned by test, not by construction comment.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        self.homes.exec_env(catalog)
    }

    /// Isolated command running this qualification under the owned homes.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which the constructor rules out. Catalog selection errors propagate.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        self.qualification
            .command(catalog)?
            .with_env(&self.homes.env(catalog))
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

#[cfg(test)]
#[path = "tool_homes_domain_tests.rs"]
mod tool_homes_domain_tests;
