use crate::errors::ContractError;
use crate::workflow::ScheduleTrigger;
use serde::{Deserialize, Serialize};

pub const UPGRADES_WORKFLOW_PATH: &str = ".github/workflows/upgrades.yml";

pub const DEFAULT_UPGRADES_CRON: &str = "17 5 * * *";

pub const DEFAULT_UPGRADES_APP_ID_VARIABLE: &str = "VELNOR_DEPS_APP_ID";

pub const DEFAULT_UPGRADES_APP_KEY_SECRET: &str = "VELNOR_DEPS_APP_PRIVATE_KEY";

pub const DEFAULT_UPGRADES_STUCK_DAYS: u8 = 3;

pub const MAX_UPGRADES_STUCK_DAYS: u8 = 30;

fn default_schedule() -> String {
    DEFAULT_UPGRADES_CRON.to_owned()
}
fn default_app_id_variable() -> String {
    DEFAULT_UPGRADES_APP_ID_VARIABLE.to_owned()
}
fn default_app_private_key_secret() -> String {
    DEFAULT_UPGRADES_APP_KEY_SECRET.to_owned()
}
const fn default_stuck_days() -> u8 {
    DEFAULT_UPGRADES_STUCK_DAYS
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradesConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_schedule")]
    pub schedule: String,

    #[serde(default = "default_app_id_variable")]
    pub app_id_variable: String,

    #[serde(default = "default_app_private_key_secret")]
    pub app_private_key_secret: String,

    #[serde(default = "default_stuck_days")]
    pub stuck_days: u8,
}

impl Default for UpgradesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule: default_schedule(),
            app_id_variable: default_app_id_variable(),
            app_private_key_secret: default_app_private_key_secret(),
            stuck_days: default_stuck_days(),
        }
    }
}

impl UpgradesConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let bad = |path: &str, problem: String| ContractError::config(file, path, problem);
        ScheduleTrigger::single(self.schedule.clone())
            .validate()
            .map_err(|_| bad("upgrades.schedule", format!("bad_cron:{}", self.schedule)))?;
        if !is_credential_name(&self.app_id_variable) {
            return Err(bad(
                "upgrades.app_id_variable",
                "invalid_variable_name".to_owned(),
            ));
        }
        if !is_credential_name(&self.app_private_key_secret) {
            return Err(bad(
                "upgrades.app_private_key_secret",
                "invalid_secret_name".to_owned(),
            ));
        }
        if !(1..=MAX_UPGRADES_STUCK_DAYS).contains(&self.stuck_days) {
            return Err(bad(
                "upgrades.stuck_days",
                format!("stuck_days_out_of_range:{}", self.stuck_days),
            ));
        }
        Ok(())
    }
}

fn is_credential_name(value: &str) -> bool {
    value.len() <= 100
        && value.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !value.starts_with("GITHUB_")
}
