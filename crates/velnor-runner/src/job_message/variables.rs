// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use serde::{Deserialize, Deserializer};

use super::step::Discriminator;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskType {
    Variable,
    Regex,
}

impl<'de> Deserialize<'de> for MaskType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Discriminator::deserialize(deserializer)?;
        if value.matches(1, "variable") {
            Ok(Self::Variable)
        } else if value.matches(2, "regex") {
            Ok(Self::Regex)
        } else {
            Err(serde::de::Error::custom("unknown mask type"))
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskHint {
    #[serde(default, rename = "type", alias = "Type")]
    pub kind: Option<MaskType>,
    #[serde(default, alias = "Value")]
    pub value: Option<Secret<String>>,
}
