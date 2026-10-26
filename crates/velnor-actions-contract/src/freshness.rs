// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

pub const FRESHNESS_CLASSES: [&str; 9] = [
    "compiler",
    "bootstrap",
    "tools",
    "crates",
    "velnor",
    "actions",
    "alint",
    "runner",
    "deferred",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshnessRequirement {
    pub class: String,

    pub max_age_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImageEvidence {
    pub image_os: String,
}

pub const UNOBSERVED_IMAGE_VALUE: &str = "unknown";

impl FreshnessRequirement {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        validate_freshness_class(&self.class)?;
        if self.max_age_days < 1 {
            return Err(ContractError::config(
                file,
                format!("freshness.{}.max_age_days", self.class),
                "must_be_at_least_one",
            ));
        }
        Ok(())
    }
}

impl RunnerImageEvidence {
    pub fn observed(os: &str) -> Result<Self, ContractError> {
        crate::cachekey::validate_semantic_text("image_os", os)?;
        if os == UNOBSERVED_IMAGE_VALUE {
            return Err(ContractError::identity("image_os", "unobserved_marker"));
        }
        Ok(Self {
            image_os: os.to_owned(),
        })
    }

    #[must_use]
    pub fn unobserved() -> Self {
        Self {
            image_os: UNOBSERVED_IMAGE_VALUE.to_owned(),
        }
    }

    #[must_use]
    pub fn is_unobserved(&self) -> bool {
        self.image_os == UNOBSERVED_IMAGE_VALUE
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        crate::cachekey::validate_semantic_text("image_os", &self.image_os)
    }
}

pub fn validate_freshness_class(class: &str) -> Result<(), ContractError> {
    if FRESHNESS_CLASSES.contains(&class) {
        Ok(())
    } else {
        Err(ContractError::identity(
            "freshness.class",
            format!("unknown_class:{class}"),
        ))
    }
}

#[must_use]
pub fn runner_family_changed(old_label: &str, new_label: &str) -> bool {
    runner_family(old_label) != runner_family(new_label)
}

fn runner_family(label: &str) -> &str {
    label.strip_suffix("-arm").unwrap_or(label)
}
