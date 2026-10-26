// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::error::MiseError;
use crate::requests::PinnedToolExec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextestDriver {
    Cargo,
    Mbx,
}

impl NextestDriver {
    #[must_use]
    pub const fn program(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }

    #[must_use]
    pub fn tools(self) -> Vec<PinnedTool> {
        match self {
            Self::Cargo | Self::Mbx => vec![PinnedTool::Rust, PinnedTool::Nextest],
        }
    }
}

pub const ARCHIVE_FILE: &str = "target/nextest/tests.tar.zst";

pub const NEXTEST_EXTRACT_BASE: &str = "$RUNNER_TEMP/velnor/nextest";

pub(crate) const NEXTEST_PROFILE: &str = "ci";

pub(crate) const CARGO_PROFILE: &str = "test";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextestPartition {
    index: u32,
    count: u32,
}

impl NextestPartition {
    pub fn new(index: u32, count: u32) -> Result<Self, MiseError> {
        if count == 0 || index > count {
            return Err(MiseError::InvalidNextestInput {
                field: "partition".to_owned(),
                value: format!("{index}_of_{count}"),
            });
        }
        Ok(Self { index, count })
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    #[must_use]
    pub const fn count(self) -> u32 {
        self.count
    }

    #[must_use]
    pub fn partition_arg(self) -> String {
        format!("hash:{}/{}", self.index, self.count)
    }
}

pub(crate) fn check_token(field: &str, value: &str) -> Result<(), MiseError> {
    let valid = !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'));
    if valid {
        Ok(())
    } else {
        Err(MiseError::InvalidNextestInput {
            field: field.to_owned(),
            value: value.to_owned(),
        })
    }
}

pub(crate) fn fixed_payload(driver: NextestDriver, args: &[&str]) -> Vec<OsString> {
    let mut payload = Vec::with_capacity(args.len() + 1);
    payload.push(OsString::from(driver.program()));
    payload.extend(args.iter().map(OsString::from));
    payload
}

pub(crate) fn nextest_exec(
    driver: NextestDriver,
    args: &[OsString],
) -> Result<PinnedToolExec, MiseError> {
    PinnedToolExec::new(driver.tools(), OsStr::new(driver.program()), args.to_vec())
}

pub(crate) fn argv_of(
    catalog: &ToolCatalog,
    driver: NextestDriver,
    payload: &[OsString],
) -> Vec<OsString> {
    let mut argv = vec![OsString::from("mise")];
    argv.extend(crate::command::mise_argv_tail(
        "exec",
        &catalog.tool_specs(&driver.tools()),
        payload,
    ));
    argv
}
