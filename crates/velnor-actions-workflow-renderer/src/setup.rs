// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepRole};

use crate::{RenderError, steps};

pub const MISE_ACTION_NAME: &str = "jdx/mise-action";
pub const SETUP_MISE_NAME: &str = "Setup Mise";
pub use velnor_actions_mise::MISE_BINARY_SHA256_LINUX_X64;
pub use velnor_actions_mise::catalog::{
    MISE_BINARY_SHA256_MACOS_ARM64, MISE_BINARY_SHA256_MACOS_X64,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseSetup {
    pub uses: String,
    pub version: String,
    pub sha256: String,
}

impl MiseSetup {
    pub fn for_target(&self, target: &str) -> Result<Self, RenderError> {
        self.validate()?;
        if self.version != velnor_actions_mise::MISE_VERSION {
            return Err(RenderError::BadCommand(format!(
                "mise_setup_unqualified_version:{}",
                self.version
            )));
        }
        let platform =
            velnor_actions_contract::ReleaseTarget::parse_triple(target).ok_or_else(|| {
                RenderError::BadCommand(format!("mise_setup_unsupported_target:{target}"))
            })?;
        let binary = velnor_actions_mise::current_mise_binary(platform);
        Ok(Self {
            version: binary.version.to_owned(),
            sha256: binary.sha256.to_owned(),
            ..self.clone()
        })
    }

    pub(crate) fn for_approved_generator(
        &self,
        lock: &velnor_actions_contract::GeneratorLock,
        target: velnor_actions_contract::ReleaseTarget,
        approved: &velnor_actions_mise::catalog::lock::ApprovedGeneratorRelease<'_>,
    ) -> Result<Self, RenderError> {
        let binary = velnor_actions_mise::catalog::lock::mise_binary_for_approved_generator(
            lock, target, approved,
        )
        .map_err(|error| RenderError::BadCommand(error.to_string()))?;
        let current = self.for_target(target.triple())?;
        Ok(Self {
            version: binary.version.to_owned(),
            sha256: binary.sha256.to_owned(),
            ..current
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
