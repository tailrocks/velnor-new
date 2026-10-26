// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::ContractError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TofuTaskKind {
    Fmt,
    InitForValidate,
    Validate,
}

impl TofuTaskKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::InitForValidate => "init",
            Self::Validate => "validate",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value {
            "fmt" => Ok(Self::Fmt),
            "init" => Ok(Self::InitForValidate),
            "validate" => Ok(Self::Validate),
            _ => Err(ContractError::identity(
                "task_kind",
                format!("unknown_kind:{value}"),
            )),
        }
    }
}
