//! Typed manual workflow dispatch inputs.
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
/// Typed `workflow_dispatch` inputs, interpreted by their workflow boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDispatch {
    /// Dispatch inputs, sorted by name, unique.
    pub inputs: Vec<DispatchInput>,
}
/// One typed dispatch input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchInput {
    /// Input name (`[a-z0-9-_]`).
    pub name: String,
    /// Input value type.
    #[serde(default)]
    pub input_type: DispatchInputType,
    /// Whether the dispatcher must supply a value.
    pub required: bool,
    /// Optional single-line display description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Closed choices; present only for choice inputs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Optional default value (ASCII, no control characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Supported native dispatch input value types.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DispatchInputType {
    /// Text input.
    #[default]
    String,
    /// Native Boolean input.
    Boolean,
    /// Selection from explicitly enumerated values.
    Choice,
}
impl DispatchInputType {
    /// GitHub Actions YAML type spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Boolean => "boolean",
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
    /// Validate names, literal defaults, descriptions, and closed choices.
    pub(super) fn validate(&self) -> Result<(), ContractError> {
        if self.name.is_empty()
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
        {
            return Err(ContractError::identity(
                "trigger.dispatch.inputs.name",
                format!("bad_name:{}", self.name),
            ));
        }
        if self
            .description
            .as_ref()
            .is_some_and(|value| value.is_empty() || !safe_literal(value))
        {
            return Err(ContractError::identity(
                "trigger.dispatch.inputs.description",
                "bad_description",
            ));
        }
        self.validate_options()?;
        if let Some(default) = &self.default {
            let valid = safe_literal(default)
                && match self.input_type {
                    DispatchInputType::String => true,
                    DispatchInputType::Boolean => matches!(default.as_str(), "true" | "false"),
                    DispatchInputType::Choice => self.options.contains(default),
                };
            if !valid {
                return Err(ContractError::identity(
                    "trigger.dispatch.inputs.default",
                    format!("bad_default:{}", self.name),
                ));
            }
        }
        Ok(())
    }

    fn validate_options(&self) -> Result<(), ContractError> {
        let unique: BTreeSet<&str> = self.options.iter().map(String::as_str).collect();
        let valid = if self.input_type == DispatchInputType::Choice {
            !self.options.is_empty()
                && unique.len() == self.options.len()
                && self
                    .options
                    .iter()
                    .all(|option| !option.is_empty() && safe_literal(option))
        } else {
            self.options.is_empty()
        };
        if !valid {
            return Err(ContractError::identity(
                "trigger.dispatch.inputs.options",
                "bad_options",
            ));
        }
        Ok(())
    }
}

fn safe_literal(value: &str) -> bool {
    value.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
