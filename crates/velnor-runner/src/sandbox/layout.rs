// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootLink {
    pub name: String,
    pub target: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub root: PathBuf,
    pub cgroup: PathBuf,
    pub uid: u32,
    pub gid: u32,
    pub home: PathBuf,
    pub work_dir: PathBuf,
    pub stage_dir: PathBuf,
    pub store_read_only: Vec<PathBuf>,
    pub root_links: Vec<RootLink>,
}
