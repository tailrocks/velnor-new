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
            Self::LintToolUnpinned(tool) => write!(
                f,
                "lint needs {tool}, which mise.toml does not pin; pin it there and run `mise lock`"
            ),
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
