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
    fn from_number(number: i64) -> Self {
        match number {
            1 => Self::Variable,
            2 => Self::Regex,
            other => Self::Unsupported(other.to_string()),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireMaskType {
    Number(i64),
    Text(String),
}

impl<'de> Deserialize<'de> for MaskKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match WireMaskType::deserialize(deserializer)? {
            WireMaskType::Number(number) => Ok(Self::from_number(number)),
            WireMaskType::Text(text) => {
                let name = text.trim();
                if name.eq_ignore_ascii_case("variable") {
                    Ok(Self::Variable)
                } else if name.eq_ignore_ascii_case("regex") {
                    Ok(Self::Regex)
                } else {
                    name.parse()
                        .map(Self::from_number)
                        .map_err(|_| de::Error::custom(format!("unknown mask type name `{text}`")))
                }
            }
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
