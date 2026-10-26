// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::ContainerCreateBody;
use bollard::query_parameters::CreateContainerOptions;

use crate::docker_spec::Mount;

use super::ResourceBudget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjection {
    pub name: String,

    pub image: String,

    pub platform: String,

    pub env: Vec<String>,

    pub cmd: Vec<String>,

    pub entrypoint: Vec<String>,

    pub user: Option<String>,

    pub working_dir: Option<String>,

    pub labels: Vec<String>,

    pub mounts: Vec<Mount>,

    pub bind_mounts: Vec<BindMount>,

    pub privileged: bool,

    pub open_stdin: bool,

    pub network_mode: Option<String>,

    pub(crate) resource_budget: ResourceBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindMount {
    pub source: String,

    pub target: String,

    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub dind_id: String,

    pub runner_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BollardCreate {
    pub options: CreateContainerOptions,

    pub config: ContainerCreateBody,
}
