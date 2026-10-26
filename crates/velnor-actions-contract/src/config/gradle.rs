// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::tofu::Utf8RepoRelDir;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) const MAX_GRADLE_TIMEOUT_MINUTES: u32 = 360;

pub(super) const MAX_GRADLE_WORKERS: u8 = 64;

pub const GRADLE_JOB_PREFIX: &str = "gradle-";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradleStackConfig {
    pub roots: Vec<Utf8RepoRelDir>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_minutes: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_workers: Option<u8>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projects: BTreeMap<String, GradleProjectConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradleProjectConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<GradleService>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradleService {
    Postgres,
}

impl GradleStackConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.roots.is_empty() {
            return Err(ContractError::config(file, "stacks.gradle.roots", "empty"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for root in &self.roots {
            if root.as_str().is_empty() || !seen.insert(root.as_str()) {
                return Err(ContractError::config(
                    file,
                    "stacks.gradle.roots",
                    format!("invalid_or_duplicate:{}", root.as_str()),
                ));
            }
        }
        if let Some(minutes) = self.timeout_minutes
            && !(1..=MAX_GRADLE_TIMEOUT_MINUTES).contains(&minutes)
        {
            return Err(ContractError::config(
                file,
                "stacks.gradle.timeout_minutes",
                format!("must_be_between_1_and_{MAX_GRADLE_TIMEOUT_MINUTES}"),
            ));
        }
        if let Some(workers) = self.max_workers
            && !(1..=MAX_GRADLE_WORKERS).contains(&workers)
        {
            return Err(ContractError::config(
                file,
                "stacks.gradle.max_workers",
                format!("must_be_between_1_and_{MAX_GRADLE_WORKERS}"),
            ));
        }
        for (path, project) in &self.projects {
            let body = path.strip_prefix(':').unwrap_or_default();
            let valid = !body.is_empty()
                && body
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'));
            if !valid {
                return Err(ContractError::config(
                    file,
                    "stacks.gradle.projects",
                    format!("invalid_project_path:{path}"),
                ));
            }
            let mut services = project.services.clone();
            services.dedup();
            if services.len() != project.services.len() {
                return Err(ContractError::config(
                    file,
                    "stacks.gradle.projects",
                    format!("duplicate_service:{path}"),
                ));
            }
        }
        Ok(())
    }
}
