//! Repository-qualified tool pins with closed acquisition and probe recipes.
use super::CheckPlatform;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

mod validation;

/// One explicit tool identity; scoped to checks selecting its ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedTool {
    /// Stable ID referenced by check tool selections.
    pub id: String,
    /// Explicit supported backend and package.
    pub backend: QualifiedToolBackend,
    /// Exact stable numeric version.
    pub version: String,
    /// Typed installation options; never raw flags or environment.
    pub options: QualifiedToolOptions,
    /// Sorted prerequisite tool IDs forming an explicit dependency DAG.
    pub depends_on: Vec<String>,
    /// Sorted per-platform qualification evidence.
    pub platforms: Vec<QualifiedToolPlatform>,
}

/// Explicit acquisition backend; repository backend settings never apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualifiedToolBackend {
    /// Supported built-in backend (`rust`, `node`, or `bun`).
    Core {
        /// Built-in backend name.
        tool: String,
    },
    /// Aqua package with an explicit owner/project[/tool] path.
    Aqua {
        /// Aqua registry package selector.
        package: String,
    },
    /// Crates.io package installed through the Cargo backend.
    Cargo {
        /// Exact crates.io package name.
        crate_name: String,
    },
}

/// Closed backend-specific options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualifiedToolOptions {
    /// Backend has no additional options.
    Default,
    /// Explicit Rust components and compilation targets.
    Rust {
        /// Sorted selected toolchain components.
        components: Vec<String>,
        /// Sorted selected compilation targets.
        targets: Vec<String>,
    },
    /// Cargo package settings with explicit source or prebuilt acquisition.
    Cargo {
        /// Whether crate default features are enabled.
        default_features: bool,
        /// Sorted explicit Cargo features.
        features: Vec<String>,
        /// Explicit offline source-build or verified prebuilt-archive strategy.
        installation: QualifiedCargoInstallation,
    },
}

/// Explicit Cargo acquisition strategy; source fallback is never implicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualifiedCargoInstallation {
    /// Locked offline compilation using all verified crates.io archives.
    Source {
        /// SHA-256 of the unpacked crate Cargo.lock.
        source_lock_sha256: String,
    },
    /// Direct extraction of exact verified prebuilt release archives.
    Prebuilt {
        /// Explicit GitHub owner/repository for binary release artifacts.
        repository: String,
    },
}

/// Qualified acquisition source archive, independently of installed bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolArtifact {
    /// Exact trusted backend release/source URL.
    pub url: String,
    /// SHA-256 of fetched archive bytes.
    pub sha256: String,
}

/// Qualification on an explicit binary platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolPlatform {
    /// Supported platform, never inferred from runner label.
    pub platform: CheckPlatform,
    /// Complete pinned acquisition artifacts for this platform.
    pub artifacts: Vec<QualifiedToolArtifact>,
    /// Exhaustive pinned dependency source archives; verified before offline builds.
    pub dependency_artifacts: Vec<QualifiedToolArtifact>,
    /// SHA-256 of canonical JSON entries sorted by relative POSIX path:
    /// directories (`path`, `kind`), files (`path`, `kind`, `sha256`, `executable`),
    /// and symlinks (`path`, `kind`, `target`). Links must resolve within the prefix.
    /// Ownership and timestamps are excluded; root entry is omitted.
    pub install_tree_sha256: String,
    /// Exact installed executable bytes and fixed version probes.
    pub executables: Vec<QualifiedToolExecutable>,
}

/// One executable within the tool's owned installation prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedToolExecutable {
    /// Command name projected onto the task PATH.
    pub name: String,
    /// Normalized POSIX path relative to the owned installation prefix.
    pub path: String,
    /// SHA-256 of installed executable bytes.
    pub sha256: String,
    /// Closed readonly probe and exact expected output.
    pub probe: QualifiedToolProbe,
}

/// Fixed readonly version probe shapes; no arbitrary executable arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualifiedToolProbe {
    /// Fixed `--version`; exact first stdout line.
    Version {
        /// Exact expected first stdout line.
        expected: String,
    },
    /// Fixed `version`; exact first stdout line.
    VersionSubcommand {
        /// Exact expected first stdout line.
        expected: String,
    },
    /// Fixed `rustc -vV`; exact complete stdout (trailing newline excluded).
    RustcVerbose {
        /// Exact expected complete stdout.
        expected: String,
    },
    /// Fixed `cargo-nextest --version`; exact first stdout line.
    CargoNextestVersion {
        /// Exact expected first stdout line.
        expected: String,
    },
}

impl QualifiedTool {
    /// Whether installation or fixed probes require a qualified Rust toolchain.
    #[must_use]
    pub fn requires_compiler(&self) -> bool {
        matches!(&self.backend, QualifiedToolBackend::Core { tool } if tool == "rust")
            || matches!(
                &self.options,
                QualifiedToolOptions::Cargo {
                    installation: QualifiedCargoInstallation::Source { .. },
                    ..
                }
            )
            || self.platforms.iter().any(|platform| {
                platform.executables.iter().any(|executable| {
                    matches!(executable.name.as_str(), "cargo" | "rustc")
                        || matches!(
                            executable.probe,
                            QualifiedToolProbe::CargoNextestVersion { .. }
                                | QualifiedToolProbe::RustcVerbose { .. }
                        )
                })
            })
    }

    /// Validate one declaration; registry validation also proves dependencies.
    /// # Errors
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        validation::validate_tool(self, file, key)
    }
}

/// Validate the complete sorted tool registry and prerequisite DAG.
/// # Errors
pub fn validate_qualified_tools(tools: &[QualifiedTool], file: &str) -> Result<(), ContractError> {
    validation::validate_registry(tools, file)
}

#[cfg(test)]
mod tests;
