use velnor_actions_contract::{Step, StepRole};

use crate::{RenderError, steps};

pub const MISE_ACTION_NAME: &str = "jdx/mise-action";
pub const SETUP_MISE_NAME: &str = "Setup Mise";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseSetup {
    pub uses: String,
    pub version: String,
    pub sha256: String,
}

pub const MISE_BINARY_SHA256_LINUX_X64: &str =
    "e97c38973336aa2ff350fea50f47b412b266fe263b102b7200ff1e9b118c6942";
pub const MISE_BINARY_SHA256_MACOS_ARM64: &str =
    "d40e2fcd96fed7c70b04af282c36534d87d0318bef89c16fa00c9f98e55ade88";
pub const MISE_BINARY_SHA256_MACOS_X64: &str =
    "72e1ced3bac89d0e0aa28b04bd22f8941c111d3b20c74a77bfb6a9486f25f7d3";

impl MiseSetup {
    pub fn for_target(&self, target: &str) -> Result<Self, RenderError> {
        self.validate()?;
        if self.version != "2026.10.18" {
            return Err(RenderError::BadCommand(format!(
                "mise_setup_unqualified_version:{}",
                self.version
            )));
        }
        let sha256 = match target {
            "x86_64-unknown-linux-gnu" => MISE_BINARY_SHA256_LINUX_X64,
            "aarch64-apple-darwin" => MISE_BINARY_SHA256_MACOS_ARM64,
            "x86_64-apple-darwin" => MISE_BINARY_SHA256_MACOS_X64,
            _ => {
                return Err(RenderError::BadCommand(format!(
                    "mise_setup_unsupported_target:{target}"
                )));
            }
        };
        Ok(Self {
            sha256: sha256.to_owned(),
            ..self.clone()
        })
    }

    pub fn validate(&self) -> Result<(), RenderError> {
        steps::validate_uses(&self.uses)?;
        if !self.uses.starts_with(&format!("{MISE_ACTION_NAME}@")) {
            return Err(RenderError::BadActionRef(format!(
                "not_mise_action:{}",
                self.uses
            )));
        }
        if !is_catalog_version(&self.version) {
            return Err(RenderError::BadCommand(format!(
                "bad_mise_version:{}",
                self.version
            )));
        }
        if !velnor_actions_contract::ids::is_lower_hex_len(&self.sha256, 64) {
            return Err(RenderError::BadCommand("bad_mise_sha256".to_owned()));
        }
        Ok(())
    }
}

pub fn mise_setup_step(setup: &MiseSetup) -> Result<Step, RenderError> {
    setup.validate()?;
    let mut step = steps::action_step(
        SETUP_MISE_NAME,
        &setup.uses,
        std::collections::BTreeMap::from([
            ("version".to_owned(), setup.version.clone()),
            ("sha256".to_owned(), setup.sha256.clone()),
            ("install".to_owned(), "false".to_owned()),
            ("env".to_owned(), "false".to_owned()),
            ("cache".to_owned(), "false".to_owned()),
            ("cache_save".to_owned(), "false".to_owned()),
        ]),
    )?;
    step.role = Some(StepRole::MiseSetup);
    Ok(step)
}

fn is_catalog_version(value: &str) -> bool {
    !value.is_empty()
        && value != "latest"
        && !value.contains("latest")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && value.contains('.')
        && !value.contains("${{")
}
