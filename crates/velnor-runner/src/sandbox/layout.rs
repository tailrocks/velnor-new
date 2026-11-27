// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{CGROUP_ROOT, GEN_ROOT, SANDBOX_ROOTS};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootLink {
    pub name: String,
    pub target: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub root: PathBuf,
    pub cgroup: PathBuf,
    pub netns: PathBuf,
    pub uid: u32,
    pub gid: u32,
    pub home: PathBuf,
    pub work_dir: PathBuf,
    pub stage_dir: PathBuf,
    pub store_read_only: Vec<PathBuf>,
    pub root_links: Vec<RootLink>,
}

#[must_use]
pub(super) fn work_dir_of(reservation: u64) -> PathBuf {
    Path::new(GEN_ROOT)
        .join("work")
        .join(reservation.to_string())
}

#[must_use]
pub(super) fn stage_dir_of(reservation: u64) -> PathBuf {
    Path::new(GEN_ROOT)
        .join("stage")
        .join(reservation.to_string())
}

#[must_use]
pub(super) fn root_of(reservation: u64) -> PathBuf {
    Path::new(SANDBOX_ROOTS).join(reservation.to_string())
}

#[must_use]
pub(super) fn cgroup_of(reservation: u64) -> PathBuf {
    Path::new(CGROUP_ROOT).join(format!("job-{reservation}"))
}
