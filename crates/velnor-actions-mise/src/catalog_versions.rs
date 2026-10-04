//! Exact-version validation plus qualification sources, split from `catalog`.
//!
//! Hosted here because the crate root is frozen: `mise::catalog` re-exports
//! the two public validators, keeping every existing path stable.

use velnor_actions_contract::{FreshnessRequirement, validate_freshness_class};

use super::PinnedTool;
use crate::error::MiseError;

/// Reject loose selectors; Java also permits numeric vendor version elements.
///
/// Java's JEP 223 version numbers allow arbitrary downstream elements. Require
/// at least three numeric elements here so major/minor requests remain loose.
/// Reject leading zeros and a zero major. These are normalized Mise selectors,
/// whose initial releases retain padding (`25.0.0`), rather than raw JEP 223
/// version numbers, which omit trailing zero elements.
///
/// # Errors
///
/// Returns [`MiseError::InvalidToolVersion`] for empty, `v`-prefixed,
/// `latest`, two-part, non-numeric, or non-Java extended versions.
pub fn validate_exact_version(tool: &str, version: &str) -> Result<(), MiseError> {
    let exact = version.split('.').collect::<Vec<_>>();
    if exact.len() < 3 || (tool != "java" && exact.len() != 3) {
        return Err(invalid_version(tool, version));
    }
    if tool == "java" && exact.first().is_some_and(|part| *part == "0") {
        return Err(invalid_version(tool, version));
    }
    for part in exact {
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (tool == "java" && part.len() > 1 && part.starts_with('0'))
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
        PinnedTool::RustDesktop => {
            format!("https://static.rust-lang.org/dist/channel-rust-{version}.toml")
        }
        PinnedTool::MrBoxington => {
            format!("https://github.com/jdx/mr-boxington/releases/tag/v{version}")
        }
        PinnedTool::Gh => format!("https://github.com/cli/cli/releases/tag/v{version}"),
        PinnedTool::Actionlint => {
            format!("https://github.com/rhysd/actionlint/releases/tag/v{version}")
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
        PinnedTool::Bun => format!("https://github.com/oven-sh/bun/releases/tag/bun-v{version}"),
        PinnedTool::Swift => {
            format!("https://github.com/swiftlang/swift/releases/tag/swift-{version}-RELEASE")
        }
        PinnedTool::Ruby => format!("https://github.com/ruby/ruby/releases/tag/v{version}"),
        PinnedTool::Reuse => format!("https://github.com/fsfe/reuse-tool/releases/tag/v{version}"),
        PinnedTool::Java => format!(
            "{}/tree/{}",
            super::qualification::JAVA_SOURCE_REPOSITORY,
            super::qualification::JAVA_SOURCE_COMMIT
        ),
        PinnedTool::CargoAudit => {
            format!("https://github.com/rustsec/rustsec/releases/tag/cargo-audit/v{version}")
        }
        PinnedTool::CargoDeny => {
            format!("https://github.com/EmbarkStudios/cargo-deny/releases/tag/{version}")
        }
        PinnedTool::CargoSemverChecks => format!(
            "{}/tree/{}",
            super::qualification::CARGO_SEMVER_CHECKS_SOURCE_REPOSITORY,
            super::qualification::CARGO_SEMVER_CHECKS_SOURCE_COMMIT
        ),
        PinnedTool::Alint => format!("https://github.com/asamarts/alint/releases/tag/v{version}"),
        PinnedTool::Boltffi => {
            format!("https://github.com/boltffi/boltffi/releases/tag/v{version}")
        }
        PinnedTool::Xcodegen => {
            format!("https://github.com/yonaskolb/XcodeGen/releases/tag/{version}")
        }
        PinnedTool::Jq => format!("https://github.com/jqlang/jq/releases/tag/jq-{version}"),
        PinnedTool::SwiftLint => {
            format!("https://github.com/realm/SwiftLint/releases/tag/{version}")
        }
        PinnedTool::Periphery => {
            format!("https://github.com/peripheryapp/periphery/releases/tag/{version}")
        }
        PinnedTool::Node => format!("https://github.com/nodejs/node/releases/tag/v{version}"),
        PinnedTool::Python => format!("https://github.com/python/cpython/tree/v{version}"),
        PinnedTool::Uv => format!("https://github.com/astral-sh/uv/releases/tag/{version}"),
        PinnedTool::Gradle => format!("https://github.com/gradle/gradle/releases/tag/v{version}"),
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
