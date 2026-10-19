//! Mandatory least-privilege protection observer binding for native tap merge.
use super::{HomebrewTapConfig, config_error, valid_app_id_variable, valid_secret_name};
use crate::ContractError;
use serde::{Deserialize, Serialize};

/// Dedicated App whose tap-scoped token may only observe branch protection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapProtectionObserver {
    /// Variable containing the separate protection observer App ID.
    pub app_id_variable: String,
    /// Secret containing that App's private key, never the writer's key.
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
