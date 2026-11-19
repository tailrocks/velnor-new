// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::ReleaseError;
use crate::error_family::FamilyError;
use crate::velnor_config::refuse_expressions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FamilyTrigger {
    WorkflowRun,
    TagPush,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FamilyOptions {
    pub(crate) trigger: Option<FamilyTrigger>,
    pub(crate) checksums_file: Option<String>,
    pub(crate) commit_file: bool,
    pub(crate) attest: bool,
}

pub(crate) fn family_options(
    has_family_tag: bool,
    trigger: Option<FamilyTrigger>,
    checksums_file: Option<String>,
    commit_file: bool,
    attest: bool,
) -> Result<FamilyOptions, ReleaseError> {
    if !has_family_tag {
        let key = if trigger.is_some() {
            Some("family_trigger")
        } else if checksums_file.is_some() {
            Some("checksums_file")
        } else if commit_file {
            Some("commit_file")
        } else if attest {
            Some("attest")
        } else {
            None
        };
        if let Some(key) = key {
            return Err(FamilyError::NeedsFamilyTag { key }.into());
        }
    }
    if let Some(name) = &checksums_file {
        refuse_expressions("checksums_file", &[name])?;
        if !valid_file_name(name) || RESERVED_FAMILY_FILES.contains(&name.as_str()) {
            return Err(FamilyError::InvalidChecksumsFile { name: name.clone() }.into());
        }
    }
    Ok(FamilyOptions {
        trigger,
        checksums_file,
        commit_file,
        attest,
    })
}

const RESERVED_FAMILY_FILES: [&str; 2] = ["SHA256SUMS", "COMMIT"];

fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['.', '-'])
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
