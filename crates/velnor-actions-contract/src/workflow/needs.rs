use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::workflow::ir::Job;
use crate::workflow::jobs::{CI_WORKFLOW_PATH, REQUIRED_DISPLAY_NAME, STALE_WORKFLOW_PATHS};

pub const NEEDS_CHANNEL_ENV: &str = "VELNOR_NEEDS_JSON";

pub const NEEDS_CHANNEL_EXPRESSION: &str = "${{ toJSON(needs) }}";

pub const NEEDS_EXPECTED_ENV: &str = "VELNOR_NEEDS_EXPECTED";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeedsConclusions {
    pub required_job: String,

    pub inventory: Vec<String>,
}

impl NeedsConclusions {
    pub fn from_finalized_jobs(
        required_job: &str,
        jobs: &BTreeMap<String, Job>,
    ) -> Result<Self, ContractError> {
        let Some(gate) = jobs.get(required_job) else {
            return Err(ContractError::identity(
                "needs.inventory",
                format!("missing_gate:{required_job}"),
            ));
        };
        let mut inventory = gate.needs.clone();
        inventory.sort();
        if inventory.is_empty() {
            return Err(ContractError::identity(
                "needs.inventory",
                "empty_inventory",
            ));
        }
        Ok(Self {
            required_job: required_job.to_owned(),
            inventory,
        })
    }

    #[must_use]
    pub fn channel_env(&self) -> (String, String) {
        (
            NEEDS_CHANNEL_ENV.to_owned(),
            NEEDS_CHANNEL_EXPRESSION.to_owned(),
        )
    }

    #[must_use]
    pub fn expected_env(&self) -> (String, String) {
        let mut inventory = self.inventory.clone();
        inventory.sort();
        (
            NEEDS_EXPECTED_ENV.to_owned(),
            serde_json::to_string(&inventory).unwrap_or_else(|_| "[]".to_owned()),
        )
    }

    #[must_use]
    pub fn gate_matches(&self, jobs: &BTreeMap<String, Job>) -> bool {
        jobs.get(&self.required_job).is_some_and(|job| {
            let mut needs = job.needs.clone();
            needs.sort();
            needs == self.inventory
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredCheckMigration {
    pub old_workflow: String,

    pub old_check: String,

    pub new_workflow: String,

    pub new_check: String,
}

impl RequiredCheckMigration {
    #[must_use]
    pub fn velnor_to_ci() -> Self {
        Self {
            old_workflow: STALE_WORKFLOW_PATHS[0].to_owned(),
            old_check: "Velnor / Required".to_owned(),
            new_workflow: CI_WORKFLOW_PATH.to_owned(),
            new_check: REQUIRED_DISPLAY_NAME.to_owned(),
        }
    }

    #[must_use]
    pub fn steps(&self) -> Vec<String> {
        vec![
            format!(
                "Merge the generator change so {} replaces {} in one commit.",
                self.new_workflow, self.old_workflow
            ),
            format!(
                "Let one {} run complete on the default branch so the {} check appears.",
                self.new_workflow, self.new_check
            ),
            format!(
                "In branch protection, require {} and remove {}; never remove the old check before the new one exists.",
                self.new_check, self.old_check
            ),
        ]
    }
}
