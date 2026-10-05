//! Exact-version validation plus qualification sources, split from `catalog`.
//!
//! Hosted here because the crate root is frozen: `mise::catalog` re-exports
//! the two public validators, keeping every existing path stable.

use velnor_actions_contract::{FreshnessRequirement, validate_freshness_class};

use super::PinnedTool;
use crate::error::MiseError;

/// Immutable Astral PBS release supplying the Linux runner Python.
pub const PYTHON_PBS_RELEASE: &str = "20261001";
/// Immutable release source for the selected PBS archive.
pub const PYTHON_PBS_SOURCE: &str =
    "https://github.com/astral-sh/python-build-standalone/releases/tag/20261001";
/// Direct x86_64 Linux GNU PBS artifact selected by the catalog.
pub const PYTHON_PBS_URL: &str = concat!(
    "https://github.com/astral-sh/python-build-standalone/releases/download/",
    "20261001/cpython-3.14.8%2B20261001-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"
);
/// SHA-256 of the exact PBS archive, verified against release metadata and bytes.
pub const PYTHON_PBS_SHA256: &str =
    "b373a4a4e4e70fc05f368c9b53d7738bf37637682b650d96c742805d2da26c32";
/// Download size of the selected PBS archive.
pub const PYTHON_PBS_SIZE_BYTES: &str = "36292523";
/// SHA-256 of `python/bin/python3.14` after the selected archive is extracted.
pub const PYTHON_BINARY_SHA256_LINUX_X64: &str =
    "4b67d7e58e4e3f58339106f9192dbd66421608dc2fcc6a705b20115ba588232b";
/// Policy mirror values for the compiled Python PBS artifact identity.
pub const PYTHON_ARTIFACT_POLICY: [(&str, &str); 11] = [
    ("provider", "http"),
    ("release", PYTHON_PBS_RELEASE),
    ("platform", "x86_64-unknown-linux-gnu"),
    ("flavor", "install_only_stripped"),
    ("source", PYTHON_PBS_SOURCE),
    ("url", PYTHON_PBS_URL),
    ("checksum", PYTHON_PBS_SHA256),
    ("size_bytes", PYTHON_PBS_SIZE_BYTES),
    ("strip_components", "1"),
    ("bin_path", "bin"),
    ("binary_sha256", PYTHON_BINARY_SHA256_LINUX_X64),
];

/// Build the only accepted Python HTTP selector at the pinned version.
pub(crate) fn python_tool_spec(version: &str) -> String {
    if version == super::PYTHON_VERSION {
        format!(concat!(
            "http:python[url={PYTHON_PBS_URL},",
            "checksum=sha256:{PYTHON_PBS_SHA256},",
            "strip_components=1,bin_path=bin]@{version}"
        ))
    } else {
        format!("python@{version}")
    }
}

/// Reject loose selectors: only exact `major.minor.patch` pins qualify.
///
/// # Errors
///
/// Returns [`MiseError::InvalidToolVersion`] for empty, `v`-prefixed,
/// `latest`, two-part, or non-numeric versions.
pub fn validate_exact_version(tool: &str, version: &str) -> Result<(), MiseError> {
    let exact = version.split('.').collect::<Vec<_>>();
    let [major, minor, patch] = exact.as_slice() else {
        return Err(invalid_version(tool, version));
    };
    for part in [major, minor, patch] {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
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
        PinnedTool::Reuse => format!("https://pypi.org/pypi/reuse/{version}/json"),
        PinnedTool::Python if version == super::PYTHON_VERSION => PYTHON_PBS_SOURCE.to_owned(),
        PinnedTool::Python => format!(
            "https://www.python.org/downloads/release/python-{}/",
            version.replace('.', "")
        ),
        PinnedTool::Uv => format!("https://github.com/astral-sh/uv/releases/tag/{version}"),
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
