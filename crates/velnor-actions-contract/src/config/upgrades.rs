//! `[upgrades]`: opt-in daily upgrade pull request plus a freshness gate.
//!
//! Owner rule: every project runs the latest stable Rust, Mise tools,
//! dependencies and Velnor, with no cooldown. When `enabled = true`, the
//! generator emits `.github/workflows/upgrades.yml` (see
//! `docs/implemented/upgrades.md`): an unprivileged job bumps everything and
//! regenerates the lockfiles and `.github/`, a separate job pushes the result
//! with a minted GitHub App token and opens or updates one pull request, and
//! a read-only gate fails while a pin lags or the pull request is stuck.
//! There is deliberately no cooldown setting.

use crate::errors::ContractError;
use crate::workflow::ScheduleTrigger;
use serde::{Deserialize, Serialize};

/// Generated upgrade workflow path.
pub const UPGRADES_WORKFLOW_PATH: &str = ".github/workflows/upgrades.yml";
/// Default schedule: daily, off the top of the hour.
pub const DEFAULT_UPGRADES_CRON: &str = "17 5 * * *";
/// Default App client-id variable (the `velnor-deps` App pattern).
pub const DEFAULT_UPGRADES_APP_ID_VARIABLE: &str = "VELNOR_DEPS_APP_ID";
/// Default App private-key secret.
pub const DEFAULT_UPGRADES_APP_KEY_SECRET: &str = "VELNOR_DEPS_APP_PRIVATE_KEY";
/// Default days an upgrade pull request may stay open before the gate fails.
pub const DEFAULT_UPGRADES_STUCK_DAYS: u8 = 3;
/// Largest accepted `stuck_days`.
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

/// `[upgrades]` table; absent or `enabled = false` emits nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradesConfig {
    /// Emit `upgrades.yml`; default off.
    #[serde(default)]
    pub enabled: bool,
    /// One five-field cron expression (UTC).
    #[serde(default = "default_schedule")]
    pub schedule: String,
    /// Repository or organization variable holding the App client id.
    #[serde(default = "default_app_id_variable")]
    pub app_id_variable: String,
    /// Repository or organization secret holding the App private key.
    #[serde(default = "default_app_private_key_secret")]
    pub app_private_key_secret: String,
    /// Days the upgrade pull request may stay open before the gate fails.
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
    /// Validate the schedule, the credential names and the gate bound.
    /// # Errors
    /// Names the key path and a machine-readable problem.
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

/// Upper-case variable or secret name (GitHub rejects the `GITHUB_` prefix).
fn is_credential_name(value: &str) -> bool {
    value.len() <= 100
        && value.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !value.starts_with("GITHUB_")
}

#[cfg(test)]
mod tests {
    use super::{UpgradesConfig, is_credential_name};

    fn parse(fields: &str) -> Result<UpgradesConfig, serde_json::Error> {
        serde_json::from_str(&format!("{{{fields}}}"))
    }

    #[test]
    fn defaults_are_off_daily_and_name_the_deps_app() {
        let config = parse("").expect("defaults");
        assert!(!config.enabled);
        assert_eq!(config, UpgradesConfig::default());
        assert_eq!(config.schedule, "17 5 * * *");
        assert_eq!(config.app_id_variable, "VELNOR_DEPS_APP_ID");
        assert_eq!(config.app_private_key_secret, "VELNOR_DEPS_APP_PRIVATE_KEY");
        assert_eq!(config.stuck_days, 3);
        assert_eq!(config.validate("c.toml"), Ok(()));
    }

    #[test]
    fn there_is_no_cooldown_or_scope_knob() {
        for key in [
            "\"cooldown\": \"none\"",
            "\"minimum_release_age\": \"1d\"",
            "\"scopes\": []",
        ] {
            assert!(parse(key).is_err(), "{key} is rejected");
        }
    }

    #[test]
    fn bad_values_fail_with_key_paths() {
        for (text, key) in [
            ("\"schedule\": \"daily\"", "upgrades.schedule"),
            ("\"schedule\": \"0 5 * *\"", "upgrades.schedule"),
            ("\"app_id_variable\": \"lower\"", "upgrades.app_id_variable"),
            (
                "\"app_private_key_secret\": \"GITHUB_KEY\"",
                "upgrades.app_private_key_secret",
            ),
            ("\"stuck_days\": 0", "upgrades.stuck_days"),
            ("\"stuck_days\": 31", "upgrades.stuck_days"),
        ] {
            let err = parse(text)
                .expect("parses")
                .validate("c.toml")
                .expect_err(text);
            assert!(err.to_string().contains(key), "{text}: {err}");
        }
    }

    #[test]
    fn credential_names_are_upper_snake_case() {
        assert!(is_credential_name("VELNOR_DEPS_APP_ID"));
        for bad in ["", "a", "1A", "A-B", "GITHUB_TOKEN"] {
            assert!(!is_credential_name(bad), "{bad}");
        }
    }
}
