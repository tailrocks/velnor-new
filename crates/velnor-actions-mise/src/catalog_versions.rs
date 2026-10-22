//! Exact-version validation plus qualification sources, split from `catalog`.
//!
//! Hosted here because the crate root is frozen: `mise::catalog` re-exports
//! the two public validators, keeping every existing path stable.

use velnor_actions_contract::{FreshnessRequirement, validate_freshness_class};

use super::PinnedTool;
use crate::error::MiseError;

/// Reject loose selectors: only exact `major.minor.patch` pins qualify.
///
/// # Errors
///
/// Returns [`MiseError::InvalidToolVersion`] for empty, `v`-prefixed,
/// `latest`, two-part, or non-numeric versions.
///
/// A project reference (`${VELNOR_TOOL_<TOOL>}`, hard decision 5) also
/// passes: it names the project's exact version, which the generated
/// resolver validates at run time. [`validate_concrete_exact_version`]
/// accepts literal pins only.
pub fn validate_exact_version(tool: &str, version: &str) -> Result<(), MiseError> {
    if super::project::is_project_tool_ref(version) {
        return Ok(());
    }
    validate_concrete_exact_version(tool, version)
}

/// Literal exact `major.minor.patch` pin; project references are rejected.
///
/// # Errors
///
/// Returns [`MiseError::InvalidToolVersion`] for any non-literal or loose version.
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

/// Build the shared rejection for a non-exact version string.
pub(crate) fn invalid_version(tool: &str, version: &str) -> MiseError {
    MiseError::InvalidToolVersion {
        tool: tool.to_owned(),
        version: version.to_owned(),
    }
}

/// Immutable qualification source for one tool at an exact version.
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
        PinnedTool::Zizmor => {
            format!("https://github.com/zizmorcore/zizmor/releases/tag/v{version}")
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

/// Enforce per-class freshness requirements in Rust (ver §2).
///
/// # Errors
///
/// Returns [`MiseError::Contract`] for an unknown class or a violated bound.
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
