//! `[stacks.dbt]`: dbt-fusion project roots, service containers and checks.
//!
//! The table is strict (unknown keys are errors) and carries no shell, argv,
//! YAML or `uses:` field: checks are a closed enum, services are a closed
//! catalog and `env` contains only closed profile-data names with inert
//! values. `dbt deps` always runs first and is not configurable.

use super::tofu::Utf8RepoRelDir;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Largest accepted job timeout, matching the other stacks.
pub(super) const MAX_DBT_TIMEOUT_MINUTES: u32 = 360;
/// Job id prefix of every generated dbt project job.
pub const DBT_JOB_PREFIX: &str = "dbt-";
/// Most `env` entries one table may declare.
pub(super) const MAX_DBT_ENV_ENTRIES: usize = 32;
/// Longest accepted `env` value.
pub(super) const MAX_DBT_ENV_VALUE_BYTES: usize = 128;

/// dbt stack options (`[stacks.dbt]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbtStackConfig {
    /// Project roots: sorted, duplicate-free; each holds `dbt_project.yml`.
    pub projects: Vec<Utf8RepoRelDir>,
    /// Service containers the job starts (closed catalog), sorted and unique.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<DbtService>,
    /// Closed profile-data inputs; project and path options are owned by Velnor.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Checks the job runs after `dbt deps`; defaults to [`DbtCheck::Parse`] alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<DbtCheck>,
    /// Job timeout in minutes; defaults to `workflow.crate_timeout_minutes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_minutes: Option<u32>,
}

/// Closed set of service containers a dbt job may start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbtService {
    /// A PostgreSQL warehouse stand-in.
    Postgres,
}

/// Closed set of dbt checks (`deps` always runs and is not listed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DbtCheck {
    /// `dbt parse`: needs no warehouse connection.
    Parse,
    /// `dbt compile --static-analysis off`: needs a reachable warehouse.
    Compile,
}

impl DbtCheck {
    /// Wire spelling of the check.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Compile => "compile",
        }
    }
}

impl DbtStackConfig {
    /// Checks the job runs: the declared set, or `parse` alone.
    #[must_use]
    pub fn effective_checks(&self) -> Vec<DbtCheck> {
        if self.checks.is_empty() {
            vec![DbtCheck::Parse]
        } else {
            self.checks.clone()
        }
    }

    /// Validate projects, services, env, checks and timeout.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        self.validate_projects(file)?;
        if let Some(minutes) = self.timeout_minutes
            && !(1..=MAX_DBT_TIMEOUT_MINUTES).contains(&minutes)
        {
            return Err(ContractError::config(
                file,
                "stacks.dbt.timeout_minutes",
                format!("must_be_between_1_and_{MAX_DBT_TIMEOUT_MINUTES}"),
            ));
        }
        require_sorted_unique(&self.services, "stacks.dbt.services", file)?;
        require_sorted_unique(&self.checks, "stacks.dbt.checks", file)?;
        if self.checks.contains(&DbtCheck::Compile) && self.services.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.dbt.checks",
                "compile_requires_a_service",
            ));
        }
        self.validate_env(file)
    }

    fn validate_projects(&self, file: &str) -> Result<(), ContractError> {
        let projects: Vec<&str> = self.projects.iter().map(Utf8RepoRelDir::as_str).collect();
        if projects.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.dbt.projects",
                "empty_projects",
            ));
        }
        let mut sorted = projects.clone();
        sorted.sort_unstable();
        if sorted != projects {
            return Err(ContractError::config(
                file,
                "stacks.dbt.projects",
                "must_be_sorted",
            ));
        }
        if sorted.iter().collect::<BTreeSet<_>>().len() != projects.len() {
            return Err(ContractError::config(
                file,
                "stacks.dbt.projects",
                "duplicate_project",
            ));
        }
        for project in &projects {
            if let Err(problem) = Utf8RepoRelDir::parse(project) {
                return Err(ContractError::config(
                    file,
                    "stacks.dbt.projects",
                    format!("bad_project:{problem:?}"),
                ));
            }
        }
        Ok(())
    }

    fn validate_env(&self, file: &str) -> Result<(), ContractError> {
        if self.env.len() > MAX_DBT_ENV_ENTRIES {
            return Err(ContractError::config(
                file,
                "stacks.dbt.env",
                "too_many_entries",
            ));
        }
        for (name, value) in &self.env {
            if !is_valid_env_name(name) {
                return Err(ContractError::config(
                    file,
                    "stacks.dbt.env",
                    format!("bad_env_name:{name}"),
                ));
            }
            if !is_inert_env_value(value) {
                return Err(ContractError::config(
                    file,
                    "stacks.dbt.env",
                    format!("bad_env_value:{name}"),
                ));
            }
        }
        Ok(())
    }
}

/// True for the closed set of environment-driven profile data inputs.
#[must_use]
pub(super) fn is_valid_env_name(name: &str) -> bool {
    matches!(
        name,
        "DBT_HOST" | "DBT_PORT" | "DBT_USER" | "DBT_PASSWORD" | "DBT_DBNAME"
    )
}

/// True for a value that can carry no expression, quote, shell or YAML syntax.
#[must_use]
pub(super) fn is_inert_env_value(value: &str) -> bool {
    value.len() <= MAX_DBT_ENV_VALUE_BYTES
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'/'))
}

fn require_sorted_unique<T: Ord + Copy>(
    items: &[T],
    key: &str,
    file: &str,
) -> Result<(), ContractError> {
    let mut sorted = items.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted != items {
        return Err(ContractError::config(
            file,
            key,
            "must_be_sorted_and_unique",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "dbt_tests.rs"]
mod tests;
