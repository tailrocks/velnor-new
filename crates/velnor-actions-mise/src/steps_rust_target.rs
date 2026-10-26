// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::mise_argv_tail;
use crate::error::MiseError;
use crate::steps::FORBIDDEN_ACKNOWLEDGED_RUSTUP;

pub const PREPARE_RUST_TARGET_STEP: &str = "Prepare Rust target";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRustTarget {
    host: String,
    target: String,
}

impl PrepareRustTarget {
    pub fn new(host: &str, target: &str) -> Result<Self, MiseError> {
        if host.is_empty() {
            return Err(invalid_input("host", host));
        }
        if target.is_empty() {
            return Err(invalid_input("target", target));
        }
        Ok(Self {
            host: host.to_owned(),
            target: target.to_owned(),
        })
    }

    #[must_use]
    pub fn step_name() -> &'static str {
        PREPARE_RUST_TARGET_STEP
    }

    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    #[must_use]
    pub fn argv(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        let specs = catalog.tool_specs(&[PinnedTool::Rust]);
        let mut argv = vec![OsString::from("mise")];
        argv.extend(mise_argv_tail("exec", &specs, &self.payload(catalog)));
        argv
    }

    fn payload(&self, catalog: &ToolCatalog) -> Vec<OsString> {
        vec![
            OsString::from(FORBIDDEN_ACKNOWLEDGED_RUSTUP),
            OsString::from("target"),
            OsString::from("add"),
            OsString::from("--toolchain"),
            OsString::from(catalog.rust_toolchain_name_for_host(&self.host)),
            OsString::from(&self.target),
        ]
    }
}

fn invalid_input(field: &str, value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}
