// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use crate::requests::PinnedToolExec;

pub const CANDIDATE_BUILD_PACKAGE: &str = "velnor-actions-cli";

pub const CANDIDATE_BUILD_BIN: &str = "velnor-actions";

const MBX_PROGRAM: &str = "mbx";

const BUILD_ARGS: [&str; 7] = [
    "build",
    "--release",
    "--locked",
    "--package",
    CANDIDATE_BUILD_PACKAGE,
    "--bin",
    CANDIDATE_BUILD_BIN,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateBuild {
    exec: PinnedToolExec,
}

impl CandidateBuild {
    pub fn new() -> Result<Self, MiseError> {
        let args = BUILD_ARGS.iter().map(OsString::from).collect();
        Ok(Self {
            exec: PinnedToolExec::new(
                vec![PinnedTool::Rust, PinnedTool::MrBoxington],
                std::ffi::OsStr::new(MBX_PROGRAM),
                args,
            )?,
        })
    }

    #[must_use]
    pub fn tools(&self) -> &[PinnedTool] {
        self.exec.tools()
    }

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        self.exec.argv(catalog)
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        self.exec.command(catalog)
    }
}
