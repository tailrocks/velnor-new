// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::PathBuf;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{
    IsolatedCommand, MISE_CARGO_HOME_ENV, MISE_RUSTUP_HOME_ENV, NO_AUTO_INSTALL_ENV,
    mise_argv_tail, toolchain_env,
};
use crate::error::MiseError;
use crate::requests::{MetadataQualification, MiseInstall};

pub const PREPARE_PINNED_TOOLS_STEP: &str = "Prepare pinned tools";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolHomes {
    rustup_home: String,
    cargo_home: String,
}

impl ToolHomes {
    #[must_use]
    pub fn runner_temp() -> Self {
        Self {
            rustup_home: "${{ runner.temp }}/velnor/rustup".to_owned(),
            cargo_home: "${{ runner.temp }}/velnor/cargo".to_owned(),
        }
    }

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

    #[must_use]
    pub fn rustup_home(&self) -> &str {
        &self.rustup_home
    }

    #[must_use]
    pub fn cargo_home(&self) -> &str {
        &self.cargo_home
    }

    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        if catalog.exports_rustup_toolchain() {
            toolchain_env(
                &self.rustup_home,
                &self.cargo_home,
                &catalog.rustup_toolchain(),
            )
        } else {
            vec![
                (
                    OsString::from(MISE_RUSTUP_HOME_ENV),
                    OsString::from(&self.rustup_home),
                ),
                (
                    OsString::from(MISE_CARGO_HOME_ENV),
                    OsString::from(&self.cargo_home),
                ),
                (
                    OsString::from("RUSTUP_HOME"),
                    OsString::from(&self.rustup_home),
                ),
                (
                    OsString::from("CARGO_HOME"),
                    OsString::from(&self.cargo_home),
                ),
            ]
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparePinnedTools {
    install: MiseInstall,
    homes: ToolHomes,
}

impl PreparePinnedTools {
    pub fn new(tools: Vec<PinnedTool>, homes: ToolHomes) -> Result<Self, MiseError> {
        Ok(Self {
            install: MiseInstall::new(tools)?,
            homes,
        })
    }

    #[must_use]
    pub fn step_name() -> &'static str {
        PREPARE_PINNED_TOOLS_STEP
    }

    #[must_use]
    pub fn tools(&self) -> &[PinnedTool] {
        self.install.tools()
    }

    #[must_use]
    pub fn homes(&self) -> &ToolHomes {
        &self.homes
    }

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        self.install.argv(catalog)
    }

    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        let mut env = IsolatedCommand::env_overlay();
        env.extend(self.homes.env(catalog));
        env
    }

    #[must_use]
    pub fn env_without_homes(&self) -> Vec<(OsString, OsString)> {
        IsolatedCommand::env_overlay()
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        self.install
            .command(catalog)?
            .with_env(&self.homes.env(catalog))
    }
}

pub const PREPARE_RUST_COMPONENTS_STEP: &str = "Prepare Rust components";

const RUST_COMPONENTS: [&str; 2] = ["clippy", "rustfmt"];

pub(crate) const FORBIDDEN_ACKNOWLEDGED_RUSTUP: &str = "rustup";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRustComponents {
    homes: ToolHomes,
}

impl PrepareRustComponents {
    #[must_use]
    pub fn new(homes: ToolHomes) -> Self {
        Self { homes }
    }

    #[must_use]
    pub fn step_name() -> &'static str {
        PREPARE_RUST_COMPONENTS_STEP
    }

    #[must_use]
    pub fn components() -> Vec<String> {
        RUST_COMPONENTS.iter().map(ToString::to_string).collect()
    }

    #[must_use]
    pub fn homes(&self) -> &ToolHomes {
        &self.homes
    }

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        let mut argv = vec![OsString::from("mise")];
        argv.extend(mise_argv_tail("exec", &specs, &Self::payload(catalog)));
        argv
    }

    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        self.homes.exec_env(catalog)
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        IsolatedCommand::mise_exec(&specs, &Self::payload(catalog))?
            .with_env(&self.homes.env(catalog))
    }

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

pub const VERIFY_PREPARED_INPUTS_STEP: &str = "Verify prepared inputs";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyPreparedInputs {
    qualification: MetadataQualification,
    homes: ToolHomes,
}

impl VerifyPreparedInputs {
    pub fn new(workspace_manifest: PathBuf, homes: ToolHomes) -> Result<Self, MiseError> {
        Ok(Self {
            qualification: MetadataQualification::new(workspace_manifest)?,
            homes,
        })
    }

    #[must_use]
    pub fn step_name() -> &'static str {
        VERIFY_PREPARED_INPUTS_STEP
    }

    #[must_use]
    pub fn workspace_manifest(&self) -> &std::path::Path {
        self.qualification.workspace_manifest()
    }

    #[must_use]
    pub fn homes(&self) -> &ToolHomes {
        &self.homes
    }

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        self.qualification.argv(catalog)
    }

    #[must_use]
    pub fn env(&self, catalog: &ToolCatalog) -> Vec<(OsString, OsString)> {
        self.homes.exec_env(catalog)
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        self.qualification
            .command(catalog)?
            .with_env(&self.homes.env(catalog))
    }
}

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

fn invalid_input(field: &str, value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}
