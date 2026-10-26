// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CheckPlatform, QualifiedToolBackend, QualifiedToolOptions, QualifiedToolProbe};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolRecipe {
    pub id: String,

    pub backend: QualifiedToolBackend,

    pub version: String,

    pub options: QualifiedToolOptions,

    pub depends_on: Vec<String>,

    pub platforms: Vec<QualifiedToolRecipePlatform>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolRecipePlatform {
    pub platform: CheckPlatform,

    pub artifacts: Vec<QualifiedToolArtifactRecipe>,

    pub dependency_artifacts: Vec<QualifiedToolArtifactRecipe>,

    pub executables: Vec<QualifiedToolExecutableRecipe>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolArtifactRecipe {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolExecutableRecipe {
    pub name: String,

    pub path: String,

    pub probe: QualifiedToolProbe,
}

impl QualifiedToolRecipe {
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        super::recipe_validation::validate_recipe(self, file, key)
    }
}
