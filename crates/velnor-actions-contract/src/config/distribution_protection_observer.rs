// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{HomebrewTapConfig, config_error, valid_app_id_variable, valid_secret_name};
use crate::ContractError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapProtectionObserver {
    pub app_id_variable: String,

    pub app_private_key_secret: String,
}

impl HomebrewTapProtectionObserver {
    pub(super) fn validate(
        &self,
        file: &str,
        key: &str,
        writer: &HomebrewTapConfig,
    ) -> Result<(), ContractError> {
        if !valid_app_id_variable(&self.app_id_variable) {
            return Err(config_error(
                file,
                key,
                "protection_observer.app_id_variable",
                "bad_app_id_variable",
            ));
        }
        if !valid_secret_name(&self.app_private_key_secret) {
            return Err(config_error(
                file,
                key,
                "protection_observer.app_private_key_secret",
                "bad_app_private_key_secret",
            ));
        }
        if self.app_id_variable == writer.app_id_variable
            || self.app_private_key_secret == writer.app_private_key_secret
        {
            return Err(config_error(
                file,
                key,
                "protection_observer",
                "observer_must_be_separate_from_writer",
            ));
        }
        Ok(())
    }
}
