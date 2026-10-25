use std::ffi::OsString;

use crate::cachekey::validate_semantic_text;
use crate::canonical::normalize_posix_path;
use crate::config::VelnorConfig;
use crate::errors::ContractError;
use crate::graph::{CachePolicy, ResourceDemand, TaskNode};
use crate::ids::validate_task_id;
use crate::propose::identity::IdentityInputs;

#[derive(Debug, Clone, PartialEq)]
pub struct ProposedTask {
    pub task_id: String,

    pub stack_id: String,

    pub component_id: String,

    pub task_kind: String,

    pub configuration: String,

    pub depends_on: Vec<String>,

    pub gated_by: Vec<String>,

    pub reads: Vec<String>,

    pub writes: Vec<String>,

    pub outputs: Vec<String>,

    pub resource: ResourceDemand,

    pub cache_policy: CachePolicy,

    pub identity: IdentityInputs,

    pub payload: Vec<OsString>,

    pub display_name: String,

    pub uses_clock: bool,

    pub uses_random: bool,

    pub no_targets: bool,

    pub runner_profile: String,
}

impl ProposedTask {
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
