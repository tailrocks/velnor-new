// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use serde::{Deserialize, Deserializer, de};

use super::wire::deserialize_null_default;
use crate::secret::Secret;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableValue {
    #[serde(default, alias = "Value")]
    pub value: Option<Secret<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        alias = "IsSecret"
    )]
    pub is_secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskKind {
    Variable,
    Regex,
    Unsupported(String),
}

impl Default for MaskKind {
    fn default() -> Self {
        Self::Unsupported("0".to_owned())
    }
}

impl MaskKind {
    fn from_number(number: i32) -> Self {
        match number {
            1 => Self::Variable,
            2 => Self::Regex,
            other => Self::Unsupported(other.to_string()),
        }
    }

    fn name_bits(token: &str) -> Option<i32> {
        let token = token.trim();
        if token.eq_ignore_ascii_case("variable") {
            Some(1)
        } else if token.eq_ignore_ascii_case("regex") {
            Some(2)
        } else {
            None
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireMaskType {
    Number(i32),
    Text(String),
}

impl<'de> Deserialize<'de> for MaskKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match WireMaskType::deserialize(deserializer)? {
            WireMaskType::Number(number) => Ok(Self::from_number(number)),
            WireMaskType::Text(text) => match text.trim().parse() {
                Ok(number) => Ok(Self::from_number(number)),
                Err(_) => text
                    .split(',')
                    .try_fold(0, |bits, token| Some(bits | Self::name_bits(token)?))
                    .map(Self::from_number)
                    .ok_or_else(|| de::Error::custom(format!("unknown mask type `{text}`"))),
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskHint {
    #[serde(default, rename = "type", alias = "Type")]
    pub kind: MaskKind,
    #[serde(default, alias = "Value")]
    pub value: Option<Secret<String>>,
}
