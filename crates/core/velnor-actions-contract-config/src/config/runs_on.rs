//! Typed `runs-on` selectors. Hosted catalog labels and scale-set labels
//! are different types; a hosted label cannot sit on a scale-set selector.
//!
//! Render order is fixed: `velnor`, then the scale-set name, then any
//! extra labels in input order. It is not alphabetical.

use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_release::targets::RUNNER_LABEL_CATALOG;

/// Product marker registered on the scale set.
pub const VELNOR_LABEL: &str = "velnor";
/// Canonical scale-set name and label.
pub const SCALE_SET_NAME: &str = "ubuntu-26.04-scale-set";
/// Platform recorded for the initial x64 profile.
pub const LINUX_AMD64: &str = "linux/amd64";
/// IR carrier prefix. The renderer never emits this text as YAML.
const SCALE_SET_TOKEN_PREFIX: &str = "scale-set:";

/// Hosted catalog label or a scale-set selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunsOn {
    /// One GitHub-hosted catalog label, rendered as a scalar.
    Hosted(String),
    /// Scale-set selector, rendered as a flow sequence.
    ScaleSet(ScaleSetSelector),
}

/// Validated scale-set name plus labels in render order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaleSetSelector {
    name: String,
    labels: Vec<String>,
}

impl ScaleSetSelector {
    /// Build a selector. Labels are reordered to the render order.
    ///
    /// # Errors
    ///
    /// Illegal characters, hosted catalog labels, duplicates, a missing
    /// `velnor` label, or a name that is not one of the labels.
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

    /// Scale-set name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Labels in render order, not alphabetical order.
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// IR token. Not valid YAML `runs-on` text.
    #[must_use]
    pub fn token(&self) -> String {
        let mut token = String::from(SCALE_SET_TOKEN_PREFIX);
        token.push_str(&self.labels.join("+"));
        token
    }

    /// Parse an IR token produced by [`Self::token`].
    ///
    /// # Errors
    ///
    /// Returns a config error when the token is not a legal selector.
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
    /// Parse a hosted label or a scale-set IR token.
    ///
    /// # Errors
    ///
    /// Illegal labels fail. Hosted catalog text stays a hosted selector.
    pub fn parse(text: &str) -> Result<Self, ContractError> {
        if text.starts_with(SCALE_SET_TOKEN_PREFIX) {
            return Ok(Self::ScaleSet(ScaleSetSelector::parse_token(text)?));
        }
        if is_legacy_hosted_label(text) {
            return Ok(Self::Hosted(text.to_owned()));
        }
        Err(illegal("runs_on", text))
    }

    /// True for a scale-set selector.
    #[must_use]
    pub fn is_scale_set(&self) -> bool {
        matches!(self, Self::ScaleSet(_))
    }

    /// Hosted scalar, when this is a hosted selector.
    #[must_use]
    pub fn hosted_label(&self) -> Option<&str> {
        match self {
            Self::Hosted(label) => Some(label.as_str()),
            Self::ScaleSet(_) => None,
        }
    }

    /// Scale-set labels in render order.
    #[must_use]
    pub fn scale_labels(&self) -> Option<&[String]> {
        match self {
            Self::ScaleSet(selector) => Some(selector.labels()),
            Self::Hosted(_) => None,
        }
    }
}

/// True when `label` is a hosted runner-catalog entry.
#[must_use]
pub fn is_hosted_catalog(label: &str) -> bool {
    RUNNER_LABEL_CATALOG.contains(&label)
}

/// Schema-1 pinned-label grammar (no `latest`, no expressions).
///
/// Kept identical so existing hosted labels still validate.
#[must_use]
pub fn is_legacy_hosted_label(label: &str) -> bool {
    !label.is_empty()
        && !label.contains("${{")
        && !label.contains("latest")
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
}

/// Reject empty, oversized, or non-label characters.
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
