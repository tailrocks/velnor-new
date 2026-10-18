//! Generic one-package stable binary release workflow.

use super::MiseSetup;
use crate::{RenderError, render::RenderedFile};
use velnor_actions_contract::{PackageReleasePlan, Step};

#[path = "schema2_consumer_binary_release_helpers.rs"]
mod helpers;
#[path = "schema2_consumer_binary_release_jobs.rs"]
mod jobs;
#[path = "schema2_consumer_binary_release_scripts.rs"]
mod scripts;

/// Inputs resolved by the orchestrator for one consumer stable release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerBinaryReleaseSpec {
    /// Generator version embedded in the generated-file marker.
    pub generator_version: String,
    /// Static one-product stable inventory shared by release runtime.
    pub plan: PackageReleasePlan,
    /// Consumer GitHub repository, derived from local origin.
    pub repository: String,
    /// Consumer's resolved default branch.
    pub default_branch: String,
    /// Repo-relative workspace-root Cargo manifest.
    pub manifest_path: String,
    /// Exact Cargo package name.
    pub package: String,
    /// Exact Cargo binary target name.
    pub bin: String,
    /// Pinned Mise setup for Linux jobs.
    pub linux_setup: MiseSetup,
    /// Pinned Mise setup for the Apple Silicon build job.
    pub macos_setup: MiseSetup,
    /// Complete Mise install argv for Rust and GitHub CLI.
    pub install_tools_argv: Vec<String>,
    /// Complete pinned GitHub CLI install argv for signer/publisher jobs.
    pub install_gh_argv: Vec<String>,
    /// Complete Mise install argv for Rust only.
    pub install_rust_argv: Vec<String>,
    /// Exact Cargo metadata command argv.
    pub metadata_argv: Vec<String>,
    /// Exact rustup target installation argv.
    pub add_target_argv: Vec<String>,
    /// Exact Cargo build argv.
    pub build_argv: Vec<String>,
    /// Exact pinned GitHub CLI argv prefix.
    pub gh_argv: Vec<String>,
    /// Verified Velnor helper acquisition for Linux signer/publisher jobs.
    pub linux_acquire: Step,
}

pub(super) fn render_consumer_binary_release(
    spec: &ConsumerBinaryReleaseSpec,
) -> Result<RenderedFile, RenderError> {
    jobs::render(spec)
}

#[cfg(test)]
#[path = "schema2_consumer_binary_release_tests.rs"]
mod tests;
