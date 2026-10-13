//! Typed `Prepare pinned tools` step requests (task §2, workflow §3).
//!
//! Both contracts name the same bootstrap step: install exact catalog
//! tools through the fixed `mise install` invocation with project
//! config, env files, hooks, and lockfile writes disabled, under
//! Velnor-owned tool homes. This module owns the fixed argv plus the
//! step env; renderers serialize, never invent.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{IsolatedCommand, NO_AUTO_INSTALL_ENV, mise_argv_tail, toolchain_env};
use crate::error::MiseError;
use crate::requests::{MetadataQualification, MiseInstall};

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
    /// Velnor-owned tool homes under runner temp (expression form).
    ///
    /// Shell `$VAR` never expands in the `env:` position that carries
    /// these paths; the `${{ runner.temp }}` expression form does.
    #[must_use]
    pub fn runner_temp() -> Self {
        Self {
            rustup_home: "${{ runner.temp }}/velnor/rustup".to_owned(),
            cargo_home: "${{ runner.temp }}/velnor/cargo".to_owned(),
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

    /// Full env for a pinned `exec` verification step.
    ///
    /// Isolation quartet, install-disable pair, plus the owned-homes
    /// triple: the step runs the prepared toolchain, and a missing tool
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
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        self.install.argv(catalog)
    }

    /// Full step env: isolation overlay plus the owned-homes triple.
    ///
    /// Matches [`Self::command`]'s spawner env exactly; the
    /// correspondence is pinned by test, not by construction comment.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        env.extend(self.homes.env(catalog));
        env
    }

    /// Step env without the owned-homes triple: isolation overlay only.
    ///
    /// Pure-tofu roles install no Rust toolchain, so their prepare
    /// step carries no rustup/cargo homes; config, env files, hooks,
    /// and lockfile writes stay disabled as in [`Self::env`]. Render
    /// only: local installs always run under owned homes, so this
    /// matches no spawner env.
    #[must_use]
    pub fn env_without_homes(&self) -> Vec<(OsString, OsString)> {
        IsolatedCommand::env_overlay()
    }

    /// Isolated command running this installation under the owned homes.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] only if the tool list were
    /// empty, which the constructor rules out.
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

/// Pinned-toolchain rustup program for the fixed rustup payloads.
///
/// The source-policy gate bans bare `"rustup"` spellings; this alias is
/// the acknowledgement: sanctioned direct `rustup` invocations spell the
/// program through this const only, fixed argv (see the type docs).
pub(crate) const FORBIDDEN_ACKNOWLEDGED_RUSTUP: &str = "rustup";

/// Fixed `rustup component add` for the pinned toolchain as one named step.
///
/// The step sets `RUSTUP_HOME` and `CARGO_HOME` to the owned Mise homes.
/// `rustup` reads those process variables. A missing toolchain fails.
/// It does not install a second toolchain under the default home.
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
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        let mut argv = vec![OsString::from("mise")];
        argv.extend(mise_argv_tail("exec", &specs, &Self::payload(catalog)));
        argv
    }

    /// Step env. It matches [`Self::command`] and sets the process homes.
    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        let mut env = self.homes.exec_env(catalog);
        env.extend(self.process_homes());
        env
    }

    /// Isolated command running this installation under the owned homes.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] only if the fixed payload were
    /// empty, which construction rules out.
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        let mut extra = self.homes.env(catalog);
        extra.extend(self.process_homes());
        IsolatedCommand::mise_exec(&specs, &Self::payload(catalog))?.with_env(&extra)
    }

    fn process_homes(&self) -> [(OsString, OsString); 2] {
        [
            (
                OsString::from("RUSTUP_HOME"),
                OsString::from(self.homes.rustup_home()),
            ),
            (
                OsString::from("CARGO_HOME"),
                OsString::from(self.homes.cargo_home()),
            ),
        ]
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
    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
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
    /// empty, which the constructor rules out.
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
