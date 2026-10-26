// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::CheckExecutor;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[path = "distribution_destinations.rs"]
mod destinations;
#[path = "distribution_protection_observer.rs"]
mod protection_observer;
#[path = "distribution_updater.rs"]
mod updater;
pub use destinations::{HomebrewTapDestination, HomebrewTapDestinations};
pub use protection_observer::HomebrewTapProtectionObserver;
pub use updater::TapUpdaterBinding;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homebrew_tap: Option<HomebrewTapConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homebrew_formula: Option<crate::homebrew_spec::HomebrewFormulaSpec>,
}

impl DistributionConfig {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.homebrew_tap.is_none() && self.homebrew_formula.is_none()
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if let Some(spec) = &self.homebrew_formula {
            spec.validate().map_err(|_| {
                ContractError::config(
                    file,
                    "distribution.homebrew_formula",
                    "invalid_homebrew_formula_spec",
                )
            })?;
        }
        if let Some(tap) = &self.homebrew_tap {
            tap.validate(file)?;
            if tap.updater.is_some() && self.homebrew_formula.is_some() {
                return Err(ContractError::config(
                    file,
                    "distribution.homebrew_tap.updater",
                    "custom_updater_and_native_formula_spec_are_ambiguous",
                ));
            }
            if tap.enabled {
                match (tap.updater.as_ref(), self.homebrew_formula.as_ref()) {
                    (Some(_), None) => {}
                    (None, Some(spec)) => {
                        if spec.channels.len() != tap.destinations.iter().count()
                            || spec.channels.iter().any(|channel| {
                                tap.destinations
                                    .get(channel.channel)
                                    .is_none_or(|destination| {
                                        destination.formula != channel.destination
                                    })
                            })
                        {
                            return Err(ContractError::config(
                                file,
                                "distribution.homebrew_tap.destinations",
                                "formula_destinations_must_match_homebrew_spec",
                            ));
                        }
                    }
                    (Some(_), Some(_)) | (None, None) => {
                        return Err(ContractError::config(
                            file,
                            if tap.updater.is_some() {
                                "distribution.homebrew_tap.updater"
                            } else {
                                "distribution.homebrew_formula"
                            },
                            if tap.updater.is_some() {
                                "custom_updater_and_native_formula_spec_are_ambiguous"
                            } else {
                                "enabled_homebrew_tap_requires_native_spec_or_custom_updater"
                            },
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapConfig {
    #[serde(default)]
    pub enabled: bool,

    pub destinations: HomebrewTapDestinations,

    pub repository: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updater: Option<TapUpdaterBinding>,

    pub environment: TapUpdateEnvironment,

    pub app_id_variable: String,

    pub app_private_key_secret: String,

    pub protection_observer: HomebrewTapProtectionObserver,

    pub merge: TapMergePolicy,

    pub review_completion: ExactHeadReviewCheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TapUpdateEnvironment {
    #[serde(rename = "tap-update")]
    TapUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TapMergePolicy {
    #[serde(rename = "squash-after-gates")]
    SquashAfterGates,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactHeadReviewCheck {
    pub check_name: String,

    pub app_id_variable: String,

    pub expected_reviewers: Vec<ReviewAppIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewAppIdentity {
    pub app_id_variable: String,
}

impl HomebrewTapConfig {
    #[must_use]
    pub const fn destination(
        &self,
        channel: crate::package_release::PackageReleaseChannel,
    ) -> Option<&HomebrewTapDestination> {
        self.destinations.get(channel)
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let key = "distribution.homebrew_tap";
        if !crate::vcs::is_safe_github_repository(&self.repository) {
            return Err(config_error(file, key, "repository", "bad_repository_slug"));
        }
        if let Some(updater) = &self.updater {
            updater.validate(file, key)?;
            if updater.runner.executor != CheckExecutor::Hosted {
                return Err(config_error(
                    file,
                    key,
                    "updater.runner.executor",
                    "tap_updater_requires_hosted_runner",
                ));
            }
        }
        self.destinations.validate(
            file,
            self.enabled,
            self.updater
                .as_ref()
                .map(|updater| updater.inputs.as_slice()),
        )?;
        if !valid_app_id_variable(&self.app_id_variable) {
            return Err(config_error(
                file,
                key,
                "app_id_variable",
                "bad_app_id_variable",
            ));
        }
        if !valid_secret_name(&self.app_private_key_secret) {
            return Err(config_error(
                file,
                key,
                "app_private_key_secret",
                "bad_app_private_key_secret",
            ));
        }
        self.protection_observer.validate(file, key, self)?;
        self.review_completion.validate(file, key)?;
        if self.app_id_variable == self.review_completion.app_id_variable
            || self
                .review_completion
                .expected_reviewers
                .iter()
                .any(|reviewer| reviewer.app_id_variable == self.app_id_variable)
        {
            return Err(config_error(
                file,
                key,
                "review_completion",
                "writer_app_must_not_complete_reviews",
            ));
        }
        Ok(())
    }
}

impl ExactHeadReviewCheck {
    fn validate(&self, file: &str, parent: &str) -> Result<(), ContractError> {
        if !valid_review_check_name(&self.check_name) {
            return Err(config_error(
                file,
                parent,
                "review_completion.check_name",
                "bad_review_check_name",
            ));
        }
        if !valid_app_id_variable(&self.app_id_variable) {
            return Err(config_error(
                file,
                parent,
                "review_completion.app_id_variable",
                "bad_review_app_id_variable",
            ));
        }
        if self.expected_reviewers.is_empty() || self.expected_reviewers.len() > 32 {
            return Err(config_error(
                file,
                parent,
                "review_completion.expected_reviewers",
                "invalid_expected_reviewers",
            ));
        }
        let mut expected = BTreeSet::new();
        for reviewer in &self.expected_reviewers {
            if !valid_app_id_variable(&reviewer.app_id_variable)
                || !expected.insert(reviewer.app_id_variable.as_str())
                || reviewer.app_id_variable == self.app_id_variable
            {
                return Err(config_error(
                    file,
                    parent,
                    "review_completion.expected_reviewers",
                    "invalid_expected_reviewer_identity",
                ));
            }
        }
        Ok(())
    }
}

fn config_error(file: &str, parent: &str, field: &str, problem: &str) -> ContractError {
    ContractError::config(file, format!("{parent}.{field}"), problem)
}

fn valid_formula_path(value: &str) -> bool {
    value.starts_with("Formula/")
        && value
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension == "rb")
        && valid_repo_path(value)
        && value.len() <= 512
}

fn valid_repo_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && value.len() <= 1024
        && value.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        })
}

pub(crate) fn valid_review_check_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.trim() == value
        && !value.contains("${{")
        && !value.chars().any(char::is_control)
}

pub(super) fn valid_app_id_variable(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_uppercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && !value.contains("TOKEN")
        && !value.contains("SECRET")
        && !value.contains("KEY")
}

pub(super) fn valid_secret_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_uppercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && value.contains("PRIVATE_KEY")
}
