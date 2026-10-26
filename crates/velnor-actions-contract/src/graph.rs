// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::canonical::{normalize_posix_path, validate_digest};
use crate::config::VelnorConfig;
use crate::errors::ContractError;
use crate::ids::validate_task_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceClass {
    Lightweight,

    Network,

    Compiler,

    Test,

    Service,

    Exclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32")]
pub struct CpuMilli(u32);

impl CpuMilli {
    pub fn new(value: u32) -> Result<Self, ContractError> {
        if value == 0 {
            return Err(ContractError::identity("resource.cpu_milli", "zero_bound"));
        }
        Ok(Self(value))
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32")]
pub struct MemoryMb(u32);

impl MemoryMb {
    pub fn new(value: u32) -> Result<Self, ContractError> {
        if value == 0 {
            return Err(ContractError::identity("resource.memory_mb", "zero_bound"));
        }
        Ok(Self(value))
    }

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDemand {
    pub class: ResourceClass,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_milli: Option<CpuMilli>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<MemoryMb>,

    #[serde(default)]
    pub needs_network: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachePolicy {
    pub allow_compilation_reuse: bool,

    pub allow_task_reuse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskNode {
    pub task_id: String,

    pub stack_id: String,

    pub component_id: String,

    pub task_kind: String,

    pub configuration: String,

    pub input_digest: String,

    pub depends_on: Vec<String>,

    pub gated_by: Vec<String>,

    pub reads: Vec<String>,

    pub writes: Vec<String>,

    pub outputs: Vec<String>,

    pub resource: ResourceDemand,

    pub lane_id: String,

    pub cache_policy: CachePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Data,

    Gate,

    Report,

    ResourceExclusion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskEdge {
    pub from: String,

    pub to: String,

    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskGraph {
    pub nodes: Vec<TaskNode>,

    pub edges: Vec<TaskEdge>,
}

impl ResourceDemand {
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

pub fn validate_plan_edges(edges: &[TaskEdge], task_ids: &[String]) -> Result<(), ContractError> {
    let known: BTreeSet<&str> = task_ids.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut last: Option<(&str, &str, u8)> = None;
    for edge in edges {
        validate_task_id(&edge.from)?;
        validate_task_id(&edge.to)?;
        if edge.from == edge.to {
            return Err(ContractError::identity("edges", "self_loop"));
        }
        for end in [edge.from.as_str(), edge.to.as_str()] {
            if !known.contains(end) {
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

pub(crate) fn check_sorted(list: &[String], field: &'static str) -> Result<(), ContractError> {
    if list.windows(2).all(|pair| pair[0] <= pair[1]) {
        Ok(())
    } else {
        Err(ContractError::identity(field, "must_be_sorted"))
    }
}

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

pub(crate) fn check_sorted_ids(list: &[String], field: &'static str) -> Result<(), ContractError> {
    for id in list {
        validate_task_id(id)?;
    }
    if list.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ContractError::identity(field, "must_be_sorted_unique"));
    }
    Ok(())
}

fn edge_kind_rank(kind: EdgeKind) -> u8 {
    match kind {
        EdgeKind::Data => 0,
        EdgeKind::Gate => 1,
        EdgeKind::Report => 2,
        EdgeKind::ResourceExclusion => 3,
    }
}
