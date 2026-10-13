//! Structural validation of the serialized plan graph.

use std::collections::BTreeSet;

use crate::errors::ContractError;
use crate::graph::{check_sorted, check_sorted_by, check_sorted_unique, validate_plan_edges};
use crate::ids::{plan_id_for_run, validate_run_key};

use super::{Plan, check_obligation_agreement};

impl Plan {
    /// Plan schema version.
    pub const SCHEMA: u32 = 1;

    /// Validate schema, plan ID, sorting, digests, and matrix entries.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != Self::SCHEMA {
            return Err(ContractError::UnsupportedSchema {
                field: "schema",
                found: self.schema.to_string(),
                expected: "1",
            });
        }
        validate_run_key(&self.run_key)?;
        if plan_id_for_run(&self.run_key)? != self.plan_id {
            return Err(ContractError::identity("plan_id", "plan_mismatch"));
        }
        crate::workflow::qualification_dispatch::validate_plan_qualification(self)?;
        self.runner.validate()?;
        check_sorted_unique(&self.task_ids, "task_ids")?;
        check_sorted_by(&self.packages, "packages", |pkg| pkg.package_id.as_str())?;
        check_sorted_by(&self.obligations, "obligations", |ob| ob.task_id.as_str())?;
        check_sorted_by(&self.matrix.include, "matrix.include", |entry| {
            entry.id.as_str()
        })?;
        let mut matrix_ids = BTreeSet::new();
        let mut matrix_keys = BTreeSet::new();
        for entry in &self.matrix.include {
            entry.validate(&self.run_key)?;
            if !matrix_ids.insert(entry.id.as_str()) {
                return Err(ContractError::Collision(format!("matrix id {}", entry.id)));
            }
            if !matrix_keys.insert(entry.matrix_key.as_str()) {
                let detail = format!("matrix key {}", entry.matrix_key);
                return Err(ContractError::Collision(detail));
            }
        }
        check_obligation_agreement(&self.task_ids, &self.obligations)?;
        for package in &self.packages {
            check_sorted(&package.reasons, "packages.reasons")?;
            check_sorted(&package.tasks, "packages.tasks")?;
        }
        for obligation in &self.obligations {
            obligation.validate()?;
        }
        validate_plan_edges(&self.edges, &self.task_ids)?;
        Ok(())
    }
}
