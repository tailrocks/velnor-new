// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::error::Reason;

impl Reason {
    pub(crate) fn fmt_config(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTestSetupRoot(root) => write!(
                f,
                "test_setup names the root {root}, which is not a Cargo root of this repository; name the folder of a root's Cargo.toml, or . for the repository root"
            ),
            Self::DuplicateGroup(name) => write!(
                f,
                "workspace_groups names the group {name} twice; a name names one group"
            ),
            Self::GroupRootTwice { name, root } => write!(
                f,
                "workspace_groups names the root {root} in the group {name} and in another group; a root belongs to at most one group"
            ),
            Self::UnknownGroupRoot(root) => write!(
                f,
                "workspace_groups names the root {root}, which is not a Cargo root of this repository; name the folder of a root's Cargo.toml, or . for the repository root"
            ),
            Self::GroupNameTaken(name) => write!(
                f,
                "workspace_groups names the group {name}, which is the name of a Cargo root of this repository; the group and the root would share one cache and one job name"
            ),
            Self::GroupTestGroups(root) => write!(
                f,
                "test_groups or test_lane names the root {root}, which belongs to a workspace group; a workspace group runs one job set for its roots, so split the root's tests another way or take the root out of the group"
            ),
            Self::GroupHelper => f.write_str(
                // helper-gate: first in tools-v0.1.96 via 61067c17fc
                "workspace_groups needs velnor-ci 0.1.96 or later, which writes the plan output of every group; run `velnor-gen pin-helper 0.1.96`",
            ),
            Self::GroupShape => f.write_str(
                "workspace_groups cannot be used with shape = \"groups\": a group's roots share one job, and the groups shape needs one archive and one test job per workspace; use shape = \"lint-test\" or leave the shape out",
            ),
            Self::Lint(fault) => write!(f, "{fault}"),
            Self::LintDuplicate(id) => write!(
                f,
                "two lint entries get the job id {id}; set id on one of them"
            ),
            Self::LintKeyForeign { kind, key } => {
                write!(f, "lint kind {kind} does not take the key {key}")
            }
            _ => Ok(()),
        }
    }
}
