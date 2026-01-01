//! Typed inputs for GitHub `workflow_dispatch` triggers.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

/// Typed `workflow_dispatch` inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDispatch {
    /// Dispatch inputs, sorted by name, unique.
    pub inputs: Vec<DispatchInput>,
}

/// One typed dispatch input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchInput {
    /// Input name (`[a-z0-9-_]`).
    pub name: String,
    /// Whether the dispatcher must supply a value.
    pub required: bool,
    /// GitHub input control type.
    #[serde(default)]
    pub input_type: DispatchInputType,
    /// Allowed values for a `choice` input, sorted and unique.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// Optional default value (ASCII, no control characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Supported `workflow_dispatch` input widgets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchInputType {
    /// Arbitrary validated single-line text.
    #[default]
    String,
    /// One value from a fixed validated list.
    Choice,
}

impl DispatchInputType {
    /// GitHub Actions YAML spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Choice => "choice",
        }
    }
}

impl WorkflowDispatch {
    /// Validate input names (charset, sorted, unique) and defaults.
    pub(super) fn validate(&self) -> Result<(), ContractError> {
        let names: Vec<&str> = self
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        if sorted != names {
            return Err(ContractError::identity(
                "trigger.dispatch.inputs",
                "must_be_sorted",
            ));
        }
        let unique: BTreeSet<&str> = names.iter().copied().collect();
        if unique.len() != names.len() {
            return Err(ContractError::identity(
                "trigger.dispatch.inputs",
                "duplicate_input",
            ));
        }
        for input in &self.inputs {
            input.validate()?;
        }
        Ok(())
    }
}

impl DispatchInput {
    /// Validate name charset and default value safety.
    fn validate(&self) -> Result<(), ContractError> {
        let name = self.name.as_str();
        let charset = name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        });
        if name.is_empty() || !charset {
            return Err(ContractError::identity(
                "trigger.dispatch.inputs.name",
                format!("bad_name:{name}"),
            ));
        }
        if let Some(default) = &self.default {
            let safe = !default.is_empty()
                && default
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() || byte == b' ');
            if !safe {
                return Err(ContractError::identity(
                    "trigger.dispatch.inputs.default",
                    format!("bad_default:{name}"),
                ));
            }
        }
        match self.input_type {
            DispatchInputType::String if !self.choices.is_empty() => Err(ContractError::identity(
                "trigger.dispatch.inputs.choices",
                "choices_require_choice_type",
            )),
            DispatchInputType::Choice => self.validate_choices(),
            DispatchInputType::String => Ok(()),
        }
    }

    /// Validate the closed, sorted values of a choice input.
    fn validate_choices(&self) -> Result<(), ContractError> {
        let valid_values = !self.choices.is_empty()
            && self.choices.iter().all(|choice| {
                !choice.is_empty()
                    && choice
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            });
        let mut sorted = self.choices.clone();
        sorted.sort_unstable();
        let canonical = sorted == self.choices
            && !sorted.windows(2).any(|pair| pair[0] == pair[1])
            && self
                .default
                .as_ref()
                .is_none_or(|default| self.choices.contains(default));
        if valid_values && canonical {
            Ok(())
        } else {
            Err(ContractError::identity(
                "trigger.dispatch.inputs.choices",
                "invalid_sorted_unique_values_or_default",
            ))
        }
    }
}
