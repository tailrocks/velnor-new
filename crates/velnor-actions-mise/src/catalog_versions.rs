// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{FreshnessRequirement, validate_freshness_class};

use super::PinnedTool;
use crate::error::MiseError;

pub fn validate_exact_version(tool: &str, version: &str) -> Result<(), MiseError> {
    if super::project::is_project_tool_ref(version) {
        return Ok(());
    }
    validate_concrete_exact_version(tool, version)
}

pub fn validate_concrete_exact_version(tool: &str, version: &str) -> Result<(), MiseError> {
    let exact = version.split('.').collect::<Vec<_>>();
    let [major, minor, patch] = exact.as_slice() else {
        return Err(invalid_version(tool, version));
    };
    for part in [major, minor, patch] {
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return Err(invalid_version(tool, version));
        }
    }
    Ok(())
}

pub(crate) fn invalid_version(tool: &str, version: &str) -> MiseError {
    MiseError::InvalidToolVersion {
        tool: tool.to_owned(),
        version: version.to_owned(),
    }
}

pub(crate) fn tool_source(tool: PinnedTool, version: &str) -> String {
    match tool {
        PinnedTool::Rust => "https://static.rust-lang.org/dist/channel-rust-stable.toml".to_owned(),
        PinnedTool::MrBoxington => {
            format!("https://github.com/jdx/mr-boxington/releases/tag/v{version}")
        }
        PinnedTool::Gh => format!("https://github.com/cli/cli/releases/tag/v{version}"),
        PinnedTool::Jactionlint => {
            format!("https://github.com/jdx/jactionlint/releases/tag/v{version}")
        }
        PinnedTool::Shellcheck => {
            format!("https://github.com/koalaman/shellcheck/releases/tag/v{version}")
        }
        PinnedTool::Nextest => {
            format!("https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-{version}")
        }
        PinnedTool::Opentofu => {
            format!("https://github.com/opentofu/opentofu/releases/tag/v{version}")
        }
        PinnedTool::ReleasePlz => format!("https://crates.io/api/v1/crates/release-plz/{version}"),
        PinnedTool::Crane => {
            format!("https://github.com/google/go-containerregistry/releases/tag/v{version}")
        }
        PinnedTool::Bun => format!("https://github.com/oven-sh/bun/releases/tag/bun-v{version}"),
    }
}

pub fn check_freshness_requirements(
    requirements: &[FreshnessRequirement],
) -> Result<(), MiseError> {
    for requirement in requirements {
        validate_freshness_class(&requirement.class).map_err(|err| MiseError::Contract {
            problem: err.to_string(),
        })?;
        requirement
            .validate("freshness")
            .map_err(|err| MiseError::Contract {
                problem: err.to_string(),
            })?;
    }
    Ok(())
}
