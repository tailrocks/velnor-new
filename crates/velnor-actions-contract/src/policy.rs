// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::config::{LATEST_RUNNER_LABEL, RUNNER_LABEL_CATALOG};
use crate::errors::ContractError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionPolicy {
    pub schema: u32,

    pub channel: String,

    pub registry: String,

    pub check_interval_hours: u32,

    pub max_exception_days: u32,

    pub github_runner_images: GithubRunnerImages,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubRunnerImages {
    pub linux_x64: RunnerInventory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerInventory {
    pub default: String,

    pub supported: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyException {
    pub held_version: String,

    pub owner: String,

    pub issue: String,

    pub reason: String,

    pub granted: String,

    pub expires: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NightlyRecord {
    pub toolchain: String,

    pub purpose: String,

    pub owner: String,

    pub qualified: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessStatus {
    Current,

    Stale,

    Missing,

    Mismatched,

    Unreviewed,

    ExpiredHold,

    LookupFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshnessEntry {
    pub component: String,

    pub current_pin: String,

    pub latest_stable: String,

    pub source_url: String,

    pub checked_at: String,

    pub status: FreshnessStatus,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exception: Option<PolicyException>,
}

impl VersionPolicy {
    pub const SCHEMA: u32 = 1;

    pub const CHANNEL: &'static str = "stable";

    pub const CHECK_INTERVAL_HOURS: u32 = 24;

    pub const MAX_EXCEPTION_DAYS: u32 = 14;

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.schema != Self::SCHEMA {
            return Err(ContractError::UnsupportedSchema {
                field: "schema",
                found: self.schema.to_string(),
                expected: "1",
            });
        }
        if self.channel != Self::CHANNEL {
            return Err(ContractError::config(file, "channel", "must_be_stable"));
        }
        if !is_registry_url(&self.registry) {
            return Err(ContractError::config(
                file,
                "registry",
                "malformed_registry",
            ));
        }
        if self.check_interval_hours == 0 || self.check_interval_hours > Self::CHECK_INTERVAL_HOURS
        {
            return Err(ContractError::config(
                file,
                "check_interval_hours",
                "weakens_policy",
            ));
        }
        if self.max_exception_days == 0 || self.max_exception_days > Self::MAX_EXCEPTION_DAYS {
            return Err(ContractError::config(
                file,
                "max_exception_days",
                "weakens_policy",
            ));
        }
        self.github_runner_images.linux_x64.validate(file)?;
        Ok(())
    }
}

impl RunnerInventory {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.default != LATEST_RUNNER_LABEL {
            return Err(ContractError::config(
                file,
                "github_runner_images.linux_x64.default",
                format!("must_equal_latest:{LATEST_RUNNER_LABEL}"),
            ));
        }
        if self.supported.is_empty() {
            return Err(ContractError::config(
                file,
                "github_runner_images.linux_x64.supported",
                "empty_supported",
            ));
        }
        for label in &self.supported {
            if !RUNNER_LABEL_CATALOG.contains(&label.as_str()) {
                return Err(ContractError::config(
                    file,
                    "github_runner_images.linux_x64.supported",
                    format!("unsupported_label:{label}"),
                ));
            }
        }
        if !self.supported.contains(&self.default) {
            return Err(ContractError::config(
                file,
                "github_runner_images.linux_x64.supported",
                "default_not_listed",
            ));
        }
        Ok(())
    }
}

impl PolicyException {
    pub fn validate(&self, file: &str, max_days: u32) -> Result<(), ContractError> {
        for (key, value) in [
            ("held_version", self.held_version.as_str()),
            ("owner", self.owner.as_str()),
            ("issue", self.issue.as_str()),
            ("reason", self.reason.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ContractError::config(
                    file,
                    format!("exception.{key}"),
                    "empty_field",
                ));
            }
        }
        let span = days_between(&self.granted, &self.expires)
            .ok_or_else(|| ContractError::config(file, "exception.expires", "malformed_date"))?;
        if span <= 0 || span > i64::from(max_days) {
            return Err(ContractError::config(
                file,
                "exception.expires",
                format!("window_must_be_within_{max_days}d"),
            ));
        }
        Ok(())
    }

    pub fn expired(&self, as_of: &str) -> Result<bool, ContractError> {
        let remaining = days_between(as_of, &self.expires)
            .ok_or_else(|| ContractError::identity("as_of", "malformed_date"))?;
        Ok(remaining < 0)
    }
}

impl NightlyRecord {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let Some(date) = self.toolchain.strip_prefix("nightly-") else {
            return Err(ContractError::config(
                file,
                "nightly.toolchain",
                "moving_nightly_forbidden",
            ));
        };
        if parse_iso_date(date).is_none() {
            return Err(ContractError::config(
                file,
                "nightly.toolchain",
                "malformed_toolchain_date",
            ));
        }
        for (key, value) in [
            ("purpose", self.purpose.as_str()),
            ("owner", self.owner.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ContractError::config(
                    file,
                    format!("nightly.{key}"),
                    "empty_field",
                ));
            }
        }
        if parse_iso_date(&self.qualified).is_none() {
            return Err(ContractError::config(
                file,
                "nightly.qualified",
                "malformed_date",
            ));
        }
        Ok(())
    }

    pub fn qualification_current(&self, as_of: &str) -> Result<bool, ContractError> {
        let age = days_between(&self.qualified, as_of)
            .ok_or_else(|| ContractError::identity("as_of", "malformed_date"))?;
        Ok((0..7).contains(&age))
    }
}

impl FreshnessEntry {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        for (key, value) in [
            ("component", self.component.as_str()),
            ("current_pin", self.current_pin.as_str()),
            ("latest_stable", self.latest_stable.as_str()),
            ("source_url", self.source_url.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ContractError::config(
                    file,
                    format!("entry.{key}"),
                    "empty_field",
                ));
            }
        }
        let dated = self
            .checked_at
            .get(..10)
            .is_some_and(|prefix| self.checked_at.len() >= 10 && parse_iso_date(prefix).is_some());
        if !dated {
            return Err(ContractError::config(
                file,
                "entry.checked_at",
                "malformed_timestamp",
            ));
        }
        if let Some(exception) = &self.exception {
            exception.validate(file, VersionPolicy::MAX_EXCEPTION_DAYS)?;
        }
        Ok(())
    }
}

fn is_registry_url(registry: &str) -> bool {
    registry.starts_with("https://")
        && registry.len() > "https://".len()
        && !registry.chars().any(char::is_whitespace)
}

#[must_use]
pub fn days_between(start: &str, end: &str) -> Option<i64> {
    let (year_a, month_a, day_a) = parse_iso_date(start)?;
    let (year_b, month_b, day_b) = parse_iso_date(end)?;
    Some(days_from_civil(year_b, month_b, day_b) - days_from_civil(year_a, month_a, day_a))
}

fn parse_iso_date(text: &str) -> Option<(i32, u32, u32)> {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    Some((year, month, day))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
    }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(if month <= 2 { year - 1 } else { year });
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = i64::from((month + 9) % 12);
    let day_of_year = (153 * month_index + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
