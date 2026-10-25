use std::collections::BTreeMap;

use serde::Serialize;
use velnor_actions_contract::config::JobAuthorityRole;

/// An authority inventory records workflow files and job authority facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobAuthorityInventory {
    /// Inventory schema version.
    pub schema_version: u32,
    /// Source commit for the locked generator release.
    pub generator_source_sha: String,
    /// Digest of the inventory content.
    pub inventory_sha256: String,
    /// Workflow files that can start from a supported event.
    pub workflows: Vec<InventoryWorkflow>,
}

/// One workflow file in the authority inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryWorkflow {
    /// Repository-relative workflow path.
    pub path: String,
    /// Digest of the exact rendered bytes.
    pub content_sha256: String,
    /// Direct reusable-workflow calls.
    pub calls: Vec<InventoryCall>,
    /// Jobs defined in this workflow file.
    pub jobs: Vec<InventoryJob>,
}

/// One local reusable-workflow call edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryCall {
    /// Repository-relative target path.
    pub path: String,
    /// Digest of the exact target workflow bytes.
    pub content_sha256: String,
}

/// One reviewed authority record for a final workflow job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryJob {
    /// Root workflow path that admits this job.
    pub root_path: String,
    /// Workflow file path that owns the job.
    pub workflow_path: String,
    /// Exact workflow job key.
    pub job_key: String,
    /// Explicit reviewed role from configuration.
    pub role: JobAuthorityRole,
    /// Trigger facts from the root workflow.
    pub contexts: Vec<InventoryContext>,
    /// True when YAML declares workflow or job permissions.
    pub permissions_declared: bool,
    /// Permission levels recorded for the job.
    pub permissions: BTreeMap<String, String>,
    /// Secret names used by the job.
    pub secrets: Vec<String>,
    /// Protected environment names used by the job.
    pub environments: Vec<String>,
    /// SHA-256 of the exact condition source.
    pub condition_sha256: Option<String>,
    /// Exact effective concurrency source.
    pub concurrency: Option<InventoryConcurrency>,
    /// Exact job dependencies.
    pub needs: Vec<String>,
}

/// Trigger facts that the host must intersect with its source policy.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryContext {
    /// GitHub event name.
    pub event: String,
    /// Exact branch filters.
    pub branch_filters: Vec<String>,
    /// Exact tag filters.
    pub tag_filters: Vec<String>,
    /// Exact pull-request base branch filters.
    pub pull_request_base_branches: Vec<String>,
}

/// Exact workflow or job concurrency source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryConcurrency {
    /// Scope of the source declaration.
    pub scope: String,
    /// Exact concurrency group source.
    pub group: String,
    /// Exact cancellation source.
    pub cancel_in_progress: String,
}
