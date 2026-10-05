//! Final-gate `needs` channel model (P01-3/4).
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;
use crate::workflow::ir::Job;

/// Env key carrying the finalized `needs` conclusions payload.
pub const NEEDS_CHANNEL_ENV: &str = "VELNOR_NEEDS_JSON";

/// GitHub expression producing the `needs` conclusions payload.
pub const NEEDS_CHANNEL_EXPRESSION: &str = "${{ toJSON(needs) }}";

/// Env key carrying the rendered expected validator inventory digest.
///
/// Static BLAKE3 digest of the finalized gate's sorted `needs` IDs,
/// emitted beside the runtime conclusions so the merge binds the
/// required inventory to the committed workflow instead of deriving it
/// from whatever the run happened to observe (a dropped validator
/// would otherwise shrink both sides of the check and pass silently).
pub const NEEDS_EXPECTED_ENV: &str = "VELNOR_NEEDS_EXPECTED";

/// Digest the exact sorted set of required-job IDs.
///
/// Each ID is length-prefixed before hashing, so arbitrary strings have
/// an unambiguous representation. Sorting makes the digest independent
/// of YAML/map iteration order; duplicates remain significant and fail
/// against GitHub's unique `needs` object keys.
#[must_use]
pub fn needs_inventory_digest(inventory: &[String]) -> String {
    let mut sorted = inventory.to_vec();
    sorted.sort();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"velnor-required-needs-v1\0");
    for job_id in sorted {
        let bytes = job_id.as_bytes();
        hasher.update(&(bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    format!("b3-{}", hasher.finalize().to_hex())
}

/// Producer model of the final-gate `needs` channel (P01-3/4).
///
/// The required validator inventory derives from the gate job's own
/// `needs` list — the exact set `toJSON(needs)` can observe at runtime.
/// Deriving from the finalized job set instead (every job except the
/// gate) admitted downstream jobs the gate cannot need without a cycle,
/// so the merge saw `needs_inventory_mismatch` on every run; the gate
/// list is the single source of truth. The renderer emits
/// [`NEEDS_CHANNEL_ENV`] with [`NEEDS_CHANNEL_EXPRESSION`] and a digest
/// of this inventory so merge conclusions match the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeedsConclusions {
    /// Required-gate job ID.
    pub required_job: String,
    /// Sorted required validator IDs (the gate's `needs`).
    pub inventory: Vec<String>,
}

impl NeedsConclusions {
    /// Derive the inventory from the gate job's `needs` list.
    ///
    /// Downstream jobs (needing the gate) and unrelated jobs are
    /// excluded: only the gate's `needs` are observable at runtime.
    /// # Errors
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

    /// Channel binding the renderer emits on the merge step.
    #[must_use]
    pub fn channel_env(&self) -> (String, String) {
        (
            NEEDS_CHANNEL_ENV.to_owned(),
            NEEDS_CHANNEL_EXPRESSION.to_owned(),
        )
    }

    /// Expected-inventory digest the renderer emits on the merge step.
    ///
    /// Static digest of the finalized `needs` IDs, so the merge compares
    /// the observed key set against the committed workflow without
    /// repeating a large JSON array in the generated workflow.
    #[must_use]
    pub fn expected_env(&self) -> (String, String) {
        (
            NEEDS_EXPECTED_ENV.to_owned(),
            needs_inventory_digest(&self.inventory),
        )
    }

    /// True when the finalized gate needs exactly this inventory.
    #[must_use]
    pub fn gate_matches(&self, jobs: &BTreeMap<String, Job>) -> bool {
        jobs.get(&self.required_job).is_some_and(|job| {
            let mut needs = job.needs.clone();
            needs.sort();
            needs == self.inventory
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Conclusions over one unsorted inventory.
    fn conclusions(inventory: &[&str]) -> NeedsConclusions {
        NeedsConclusions {
            required_job: "required".to_owned(),
            inventory: inventory.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn expected_env_emits_order_independent_inventory_digest() {
        let (key, value) = conclusions(&["zizmor", "plan", "alint"]).expected_env();
        assert_eq!(key, NEEDS_EXPECTED_ENV);
        assert_eq!(
            value,
            needs_inventory_digest(&["alint".to_owned(), "plan".to_owned(), "zizmor".to_owned(),])
        );
        assert!(crate::is_valid_digest(&value));
        assert_ne!(value, needs_inventory_digest(&["plan".to_owned()]));
    }

    /// Minimal job with the given `needs` for inventory tests.
    fn job_with_needs(needs: &[&str]) -> Job {
        Job {
            display_name: "Test".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: crate::workflow::timeout::JobTimeout::VALIDATOR,
            needs: needs.iter().map(ToString::to_string).collect(),
            condition: None,
            permissions: None,
            environment: None,
            steps: Vec::new(),
        }
    }

    #[test]
    fn inventory_comes_from_gate_needs_not_job_set() {
        let mut jobs = BTreeMap::new();
        jobs.insert("plan".to_owned(), job_with_needs(&[]));
        jobs.insert("required".to_owned(), job_with_needs(&["plan"]));
        jobs.insert("publish-baseline".to_owned(), job_with_needs(&["required"]));
        jobs.insert("unrelated".to_owned(), job_with_needs(&[]));
        let conclusions = NeedsConclusions::from_finalized_jobs("required", &jobs)
            .expect("gate with needs derives");
        assert_eq!(conclusions.inventory, vec!["plan".to_owned()]);
        assert!(conclusions.gate_matches(&jobs));
        let (_, value) = conclusions.expected_env();
        assert_eq!(value, needs_inventory_digest(&["plan".to_owned()]));
    }

    #[test]
    fn inventory_rejects_missing_gate_and_empty_needs() {
        let mut jobs = BTreeMap::new();
        jobs.insert("plan".to_owned(), job_with_needs(&[]));
        let Err(err) = NeedsConclusions::from_finalized_jobs("required", &jobs) else {
            panic!("missing gate fails");
        };
        assert!(err.to_string().contains("missing_gate"), "{err}");
        jobs.insert("required".to_owned(), job_with_needs(&[]));
        let Err(err) = NeedsConclusions::from_finalized_jobs("required", &jobs) else {
            panic!("empty needs fail");
        };
        assert!(err.to_string().contains("empty_inventory"), "{err}");
    }

    #[test]
    fn expected_env_handles_edge_inventories() {
        let (key, value) = conclusions(&["plan"]).expected_env();
        assert_eq!(
            (key.as_str(), value.as_str()),
            (
                NEEDS_EXPECTED_ENV,
                needs_inventory_digest(&["plan".to_owned()]).as_str()
            )
        );
        let (_, value) = conclusions(&[]).expected_env();
        assert_eq!(value, needs_inventory_digest(&[]));
    }
}
