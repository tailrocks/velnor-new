//! Neutral task proposals: adapter obligations before digests.
//!
//! [`ProposedTask`] carries the [`TaskNode`](crate::graph::TaskNode)
//! fields an adapter can propose (everything except the
//! orchestrator-computed `input_digest`/`lane_id`) plus the adapter
//! facts downstream planning needs (identity preimage, payload,
//! display, reuse signals).
//!
//! Why not [`TaskNode`](crate::graph::TaskNode) directly (§4-step-4
//! reuse-or-justify): `TaskNode` is the validated *planned* node in the
//! serialized graph wire format (`deny_unknown_fields`), requiring
//! `input_digest`/`lane_id` digests the orchestrator derives at plan
//! time from argv, toolchain, platform, closure, generator, and the
//! checkout-bound extension. An adapter cannot construct a valid one
//! (placeholders would violate validated construction), and the
//! proposal-only facts (payload, environment, unit identity, runner
//! profile, display) cannot extend the wire struct without breaking
//! compatibility. `ProposedTask` is valid at construction;
//! [`into_task_node`](ProposedTask::into_task_node) plus
//! [`validate`](crate::graph::TaskNode::validate) yields the node.

use std::ffi::OsString;

use crate::cachekey::validate_semantic_text;
use crate::canonical::normalize_posix_path;
use crate::config::VelnorConfig;
use crate::errors::ContractError;
use crate::graph::{CachePolicy, ResourceDemand, TaskNode};
use crate::ids::validate_task_id;
use crate::propose::identity::IdentityInputs;

/// One adapter-proposed task, before orchestrator digests.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedTask {
    /// Stable task ID.
    pub task_id: String,
    /// Registered detector ID.
    pub stack_id: String,
    /// Detector-defined stable component identity.
    pub component_id: String,
    /// Detector-defined task kind.
    pub task_kind: String,
    /// Toolchain/components/target/features/profile/flags/schema.
    pub configuration: String,
    /// Data producers that must finish first (sorted).
    pub depends_on: Vec<String>,
    /// Required quality gates (sorted).
    pub gated_by: Vec<String>,
    /// Paths and external resources read.
    pub reads: Vec<String>,
    /// Paths and external resources mutated.
    pub writes: Vec<String>,
    /// Reports, binaries, files, or artifacts required downstream.
    pub outputs: Vec<String>,
    /// Resource class and bounded demand.
    pub resource: ResourceDemand,
    /// Cache-reuse policy.
    pub cache_policy: CachePolicy,
    /// Complete neutral identity preimage.
    pub identity: IdentityInputs,
    /// Adapter-proposed fixed command payload (wrapped, never edited).
    pub payload: Vec<OsString>,
    /// Human/display name of the owning unit (display only; may be empty
    /// for unit-less scopes).
    pub display_name: String,
    /// Task reads the wall clock.
    pub uses_clock: bool,
    /// Task needs randomness.
    pub uses_random: bool,
    /// Adapter found no actionable target; emit no command.
    pub no_targets: bool,
    /// Resolved test-runner profile name (adapter spelling, opaque).
    pub runner_profile: String,
}

impl ProposedTask {
    /// Validate IDs, ordering, paths, resources, and identity inputs.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for malformed proposals.
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_task_id(&self.task_id)?;
        if !VelnorConfig::REGISTERED_STACKS.contains(&self.stack_id.as_str()) {
            return Err(ContractError::identity("stack_id", "unregistered_stack"));
        }
        for (field, value) in [
            ("component_id", self.component_id.as_str()),
            ("task_kind", self.task_kind.as_str()),
            ("configuration", self.configuration.as_str()),
        ] {
            validate_semantic_text(field, value)?;
        }
        crate::graph::check_sorted_ids(&self.depends_on, "depends_on")?;
        crate::graph::check_sorted_ids(&self.gated_by, "gated_by")?;
        for path in self.reads.iter().chain(&self.writes) {
            if path.trim().is_empty() {
                return Err(ContractError::identity("reads_writes", "empty_path"));
            }
        }
        for output in &self.outputs {
            normalize_posix_path(output)?;
        }
        self.resource.validate()?;
        if self.payload.is_empty() {
            return Err(ContractError::identity("payload", "empty_payload"));
        }
        validate_semantic_text("runner_profile", &self.runner_profile)?;
        self.identity.validate()?;
        Ok(())
    }

    /// Complete the planned node with orchestrator-computed digests.
    #[must_use]
    pub fn into_task_node(self, input_digest: String, lane_id: String) -> TaskNode {
        TaskNode {
            task_id: self.task_id,
            stack_id: self.stack_id,
            component_id: self.component_id,
            task_kind: self.task_kind,
            configuration: self.configuration,
            input_digest,
            depends_on: self.depends_on,
            gated_by: self.gated_by,
            reads: self.reads,
            writes: self.writes,
            outputs: self.outputs,
            resource: self.resource,
            lane_id,
            cache_policy: self.cache_policy,
        }
    }
}

#[cfg(test)]
mod tests;
