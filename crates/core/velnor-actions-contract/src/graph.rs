//! Stack-neutral task graph: typed nodes, edges, resources (par §3, §6).
//!
//! The planner MUST construct this typed graph before rendering. Nodes carry
//! the 13 required fields; edges distinguish data, gate, report/artifact,
//! and resource-exclusion relationships.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::canonical::{normalize_posix_path, validate_digest};
use crate::config::VelnorConfig;
use crate::errors::ContractError;
use crate::ids::validate_task_id;

/// Resource classes, at minimum (par §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceClass {
    /// Cheap local work.
    Lightweight,
    /// Network-dependent work.
    Network,
    /// Compiler CPU/memory work.
    Compiler,
    /// Test CPU work.
    Test,
    /// Service-backed tests.
    Service,
    /// Mutually exclusive work.
    Exclusive,
}

/// Validated CPU bound in milli-cores: finite and positive (`Some(0)` is
/// unrepresentable; use `None` for unbounded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32")]
pub struct CpuMilli(u32);

impl CpuMilli {
    /// Build a positive CPU bound.
    /// # Errors
    pub fn new(value: u32) -> Result<Self, ContractError> {
        if value == 0 {
            return Err(ContractError::identity("resource.cpu_milli", "zero_bound"));
        }
        Ok(Self(value))
    }

    /// Unwrap the bound.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for CpuMilli {
    type Error = ContractError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Validated memory bound in MiB: finite and positive (`Some(0)` is
/// unrepresentable; use `None` for unbounded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32")]
pub struct MemoryMb(u32);

impl MemoryMb {
    /// Build a positive memory bound.
    /// # Errors
    pub fn new(value: u32) -> Result<Self, ContractError> {
        if value == 0 {
            return Err(ContractError::identity("resource.memory_mb", "zero_bound"));
        }
        Ok(Self(value))
    }

    /// Unwrap the bound.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for MemoryMb {
    type Error = ContractError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Bounded resource demand for one node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDemand {
    /// Resource class.
    pub class: ResourceClass,
    /// CPU bound in milli-cores, when bounded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_milli: Option<CpuMilli>,
    /// Memory bound in MiB, when bounded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<MemoryMb>,
    /// Whether the task needs network access.
    #[serde(default)]
    pub needs_network: bool,
    /// Required service name, when service-backed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

/// Cache-reuse policy for one node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachePolicy {
    /// Whether compilation reuse is allowed.
    pub allow_compilation_reuse: bool,
    /// Whether task-result reuse is allowed.
    pub allow_task_reuse: bool,
}

/// One typed task node with the 13 required fields (par §3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskNode {
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
    /// Digest of every declared semantic input.
    pub input_digest: String,
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
    /// Stable isolated-lane identity digest.
    pub lane_id: String,
    /// Cache-reuse policy.
    pub cache_policy: CachePolicy,
}

/// Edge relationship kinds (par §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Data dependency: output flows into input.
    Data,
    /// Quality gate: must pass, produces no input.
    Gate,
    /// Report/artifact dependency.
    Report,
    /// Resource exclusion: MUST NOT overlap.
    ResourceExclusion,
}

/// One typed edge between two node task IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEdge {
    /// Source task ID.
    pub from: String,
    /// Target task ID.
    pub to: String,
    /// Relationship kind.
    pub kind: EdgeKind,
}

/// The planner's typed task graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskGraph {
    /// Task nodes, sorted by `task_id`.
    pub nodes: Vec<TaskNode>,
    /// Edges between nodes.
    pub edges: Vec<TaskEdge>,
}

impl ResourceDemand {
    /// Validate the service name (bounds are pre-validated newtypes).
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.class != ResourceClass::Service && self.service.is_some() {
            return Err(ContractError::identity(
                "resource.service",
                "service_requires_service_class",
            ));
        }
        if self
            .service
            .as_deref()
            .is_some_and(|name| name.trim().is_empty())
        {
            return Err(ContractError::identity("resource.service", "empty_name"));
        }
        Ok(())
    }
}

impl TaskNode {
    /// Validate IDs, digests, ordering, paths, and resource demand.
    /// # Errors
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
            crate::cachekey::validate_semantic_text(field, value)?;
        }
        validate_digest(&self.input_digest)?;
        validate_digest(&self.lane_id)?;
        check_sorted_ids(&self.depends_on, "depends_on")?;
        check_sorted_ids(&self.gated_by, "gated_by")?;
        for path in self.reads.iter().chain(&self.writes) {
            if path.trim().is_empty() {
                return Err(ContractError::identity("reads_writes", "empty_path"));
            }
        }
        for output in &self.outputs {
            normalize_posix_path(output)?;
        }
        self.resource.validate()?;
        Ok(())
    }
}

impl TaskGraph {
    /// Validate nodes, edges, references, and deterministic ordering.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        let mut ids = BTreeSet::new();
        let mut last: Option<&str> = None;
        for node in &self.nodes {
            node.validate()?;
            if !ids.insert(node.task_id.as_str()) {
                return Err(ContractError::identity("nodes", "duplicate_task_id"));
            }
            if last.is_some_and(|prev| prev >= node.task_id.as_str()) {
                return Err(ContractError::identity("nodes", "must_be_sorted"));
            }
            last = Some(node.task_id.as_str());
        }
        for node in &self.nodes {
            for dep in node.depends_on.iter().chain(&node.gated_by) {
                if !ids.contains(dep.as_str()) {
                    return Err(ContractError::identity(
                        "depends_on",
                        format!("unknown_task:{dep}"),
                    ));
                }
            }
        }
        let mut edges = BTreeSet::new();
        for edge in &self.edges {
            validate_task_id(&edge.from)?;
            validate_task_id(&edge.to)?;
            if edge.from == edge.to {
                return Err(ContractError::identity("edges", "self_loop"));
            }
            if !ids.contains(edge.from.as_str()) || !ids.contains(edge.to.as_str()) {
                return Err(ContractError::identity("edges", "unknown_endpoint"));
            }
            let key = (
                edge.from.as_str(),
                edge.to.as_str(),
                edge_kind_rank(edge.kind),
            );
            if !edges.insert(key) {
                return Err(ContractError::identity("edges", "duplicate_edge"));
            }
        }
        Ok(())
    }
}

/// Validate plan-level edges against the obligation task IDs.
///
/// Endpoints must name planned obligations, self-loops are rejected, and
/// edges must be sorted and unique by (`from`, `to`, kind).
/// # Errors
pub fn validate_plan_edges(edges: &[TaskEdge], task_ids: &[String]) -> Result<(), ContractError> {
    let mut seen = BTreeSet::new();
    let mut last: Option<(&str, &str, u8)> = None;
    for edge in edges {
        validate_task_id(&edge.from)?;
        validate_task_id(&edge.to)?;
        if edge.from == edge.to {
            return Err(ContractError::identity("edges", "self_loop"));
        }
        for end in [&edge.from, &edge.to] {
            if !task_ids.iter().any(|id| id == end) {
                return Err(ContractError::identity("edges", "unknown_task"));
            }
        }
        let key = (
            edge.from.as_str(),
            edge.to.as_str(),
            edge_kind_rank(edge.kind),
        );
        if !seen.insert(key) {
            return Err(ContractError::identity("edges", "duplicate_edge"));
        }
        if last.is_some_and(|prev| prev >= key) {
            return Err(ContractError::identity("edges", "must_be_sorted"));
        }
        last = Some(key);
    }
    Ok(())
}

/// Check a string list is sorted and duplicate-free.
pub(crate) fn check_sorted_unique(
    list: &[String],
    field: &'static str,
) -> Result<(), ContractError> {
    check_sorted(list, field)?;
    let unique: BTreeSet<&str> = list.iter().map(String::as_str).collect();
    if unique.len() != list.len() {
        return Err(ContractError::identity(field, "duplicate_entry"));
    }
    Ok(())
}

/// Check a string list is sorted.
pub(crate) fn check_sorted(list: &[String], field: &'static str) -> Result<(), ContractError> {
    if list.windows(2).all(|pair| pair[0] <= pair[1]) {
        Ok(())
    } else {
        Err(ContractError::identity(field, "must_be_sorted"))
    }
}

/// Check records are sorted by a key.
pub(crate) fn check_sorted_by<T>(
    list: &[T],
    field: &'static str,
    key: impl Fn(&T) -> &str,
) -> Result<(), ContractError> {
    if list.windows(2).all(|pair| key(&pair[0]) <= key(&pair[1])) {
        Ok(())
    } else {
        Err(ContractError::identity(field, "must_be_sorted"))
    }
}

/// Check a task-ID list is sorted, unique, and well-formed.
pub(crate) fn check_sorted_ids(list: &[String], field: &'static str) -> Result<(), ContractError> {
    for id in list {
        validate_task_id(id)?;
    }
    if list.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ContractError::identity(field, "must_be_sorted_unique"));
    }
    Ok(())
}

/// Rank edge kinds for duplicate detection.
fn edge_kind_rank(kind: EdgeKind) -> u8 {
    match kind {
        EdgeKind::Data => 0,
        EdgeKind::Gate => 1,
        EdgeKind::Report => 2,
        EdgeKind::ResourceExclusion => 3,
    }
}
