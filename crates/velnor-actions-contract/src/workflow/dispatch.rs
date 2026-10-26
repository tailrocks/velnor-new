// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDispatch {
    pub inputs: Vec<DispatchInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchInput {
    pub name: String,

    pub required: bool,

    #[serde(default)]
    pub input_type: DispatchInputType,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchInputType {
    #[default]
    String,

    Choice,
}

impl DispatchInputType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Choice => "choice",
        }
    }
}

impl WorkflowDispatch {
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
