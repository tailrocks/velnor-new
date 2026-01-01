//! Shard vocabulary shared with merge: proofs, budgets, test identity.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{canonical_json_bytes, digest_b3};

/// One selected test: package, target, features, binary, name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TestIdentity {
    /// Cargo package ID.
    pub package: String,
    /// Cargo target name.
    pub target: String,
    /// Sorted enabled features.
    pub features: Vec<String>,
    /// Test binary name.
    pub binary: String,
    /// Test name.
    pub name: String,
}

impl TestIdentity {
    /// Validate fields: nonempty, relative, sorted features.
    /// # Errors
    pub fn validate(&self) -> Result<(), String> {
        let fields = [&self.package, &self.target, &self.binary, &self.name];
        let clean = fields
            .iter()
            .all(|v| !v.trim().is_empty() && !v.starts_with('/'));
        let sorted = self.features.windows(2).all(|pair| pair[0] <= pair[1]);
        if clean && sorted {
            Ok(())
        } else {
            Err("malformed_test_identity".into())
        }
    }
}

/// Digest over the canonical sorted test inventory.
#[must_use]
pub fn inventory_digest(tests: &[TestIdentity]) -> String {
    let mut sorted = tests.to_vec();
    sorted.sort();
    canonical_json_bytes(&sorted)
        .map(|bytes| digest_b3(&bytes))
        .unwrap_or_default()
}

/// Merge-time shard proof binding one partition to its plan obligation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardProof {
    /// Full shard task ID with its suffix.
    pub task_id: String,
    /// Plan obligation input digest binding the selected inventory.
    pub input_digest: String,
    /// Detected test runner.
    pub runner: String,
    /// One-based shard index.
    pub shard_index: u32,
    /// Total shard count.
    pub shard_count: u32,
    /// Sorted tests assigned to this shard.
    pub tests: Vec<TestIdentity>,
    /// Digest over the canonical sorted full inventory.
    pub inventory_digest: String,
    /// Archive digest (Nextest only).
    pub archive_digest: Option<String>,
    /// Metadata proves the target carries no applicable tests.
    pub no_test_targets: bool,
}

/// Configured resource limits revalidated at merge time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Compiler process budget.
    pub compiler_budget: u32,
    /// Test process budget.
    pub test_budget: u32,
    /// Matrix `max-parallel` setting.
    pub max_parallel: u32,
    /// Known runner capacity.
    pub capacity: u32,
    /// Total requested shards.
    pub shards: u32,
    /// Configured retries (V1: zero).
    pub retries: u32,
    /// Shared-service resource groups, when measured (PAR-8.19).
    #[serde(default)]
    pub resource_groups: Vec<String>,
}
