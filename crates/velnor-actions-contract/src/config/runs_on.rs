// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::workflow::RUNNER_LABEL_CATALOG;
use crate::errors::ContractError;

pub const VELNOR_LABEL: &str = "velnor";

pub const SCALE_SET_NAME: &str = "ubuntu-26.04-scale-set";

pub const LINUX_AMD64: &str = "linux/amd64";

const SCALE_SET_TOKEN_PREFIX: &str = "scale-set:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunsOn {
    Hosted(String),

    ScaleSet(ScaleSetSelector),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaleSetSelector {
    name: String,
    labels: Vec<String>,
}

impl ScaleSetSelector {
    pub fn try_new(name: &str, labels: &[String]) -> Result<Self, ContractError> {
        validate_label(name, "execution.profiles.scale_set.name")?;
        if name == VELNOR_LABEL || is_hosted_catalog(name) {
            return Err(illegal("execution.profiles.scale_set.name", name));
        }
        let mut seen = Vec::new();
        for label in labels {
            validate_label(label, "execution.profiles.scale_set.labels")?;
            if is_hosted_catalog(label) {
                return Err(illegal("execution.profiles.scale_set.labels", label));
            }
            if seen.iter().any(|have: &String| have == label) {
                return Err(ContractError::config(
                    "config.toml",
                    "execution.profiles.scale_set.labels",
                    format!("duplicate_label:{label}"),
                ));
            }
            seen.push(label.clone());
        }
        if !seen.iter().any(|label| label == VELNOR_LABEL) {
            return Err(ContractError::config(
                "config.toml",
                "execution.profiles.scale_set.labels",
                "missing_velnor_label",
            ));
        }
        if !seen.iter().any(|label| label == name) {
            return Err(ContractError::config(
                "config.toml",
                "execution.profiles.scale_set.labels",
                "missing_scale_set_label",
            ));
        }
        let mut ordered = vec![VELNOR_LABEL.to_owned(), name.to_owned()];
        for label in labels {
            if label != VELNOR_LABEL && label != name {
                ordered.push(label.clone());
            }
        }
        Ok(Self {
            name: name.to_owned(),
            labels: ordered,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    #[must_use]
    pub fn token(&self) -> String {
        let mut token = String::from(SCALE_SET_TOKEN_PREFIX);
        token.push_str(&self.labels.join("+"));
        token
    }

    pub fn parse_token(text: &str) -> Result<Self, ContractError> {
        let Some(body) = text.strip_prefix(SCALE_SET_TOKEN_PREFIX) else {
            return Err(illegal("runs_on", text));
        };
        let labels: Vec<String> = body.split('+').map(str::to_owned).collect();
        let Some(name) = labels.get(1) else {
            return Err(illegal("runs_on", text));
        };
        let name = name.clone();
        Self::try_new(&name, &labels)
    }
}

impl RunsOn {
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        if text.starts_with(SCALE_SET_TOKEN_PREFIX) {
            return Ok(Self::ScaleSet(ScaleSetSelector::parse_token(text)?));
        }
        if is_legacy_hosted_label(text) {
            return Ok(Self::Hosted(text.to_owned()));
        }
        Err(illegal("runs_on", text))
    }

    #[must_use]
    pub fn is_scale_set(&self) -> bool {
        matches!(self, Self::ScaleSet(_))
    }

    #[must_use]
    pub fn hosted_label(&self) -> Option<&str> {
        match self {
            Self::Hosted(label) => Some(label.as_str()),
            Self::ScaleSet(_) => None,
        }
    }

    #[must_use]
    pub fn scale_labels(&self) -> Option<&[String]> {
        match self {
            Self::ScaleSet(selector) => Some(selector.labels()),
            Self::Hosted(_) => None,
        }
    }
}

#[must_use]
pub fn is_hosted_catalog(label: &str) -> bool {
    RUNNER_LABEL_CATALOG.contains(&label)
}

#[must_use]
pub fn is_legacy_hosted_label(label: &str) -> bool {
    !label.is_empty()
        && !label.contains("${{")
        && !label.contains("latest")
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
}

fn validate_label(label: &str, key: &str) -> Result<(), ContractError> {
    let chars_ok = (1..=64).contains(&label.len())
        && !label.starts_with('-')
        && !label.ends_with('-')
        && !label.contains("..")
        && label.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        });
    if chars_ok {
        Ok(())
    } else {
        Err(illegal(key, label))
    }
}

fn illegal(key: &str, label: &str) -> ContractError {
    ContractError::config("config.toml", key, format!("illegal_label:{label}"))
}
