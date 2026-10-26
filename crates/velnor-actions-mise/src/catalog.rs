// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::MiseError;

#[path = "lock.rs"]
pub mod lock;

#[path = "lock_verify.rs"]
mod lock_verify;

#[path = "release_plz.rs"]
pub mod release_plz;

#[path = "catalog_mbx.rs"]
pub mod mbx;
pub use mbx::MbxProvisioning;

#[path = "catalog_source_build_bootstrap.rs"]
pub mod source_build_bootstrap;

#[path = "catalog_identity.rs"]
mod identity;

#[path = "catalog_owned_source.rs"]
pub mod owned_source;

#[path = "catalog_versions.rs"]
mod versions;
pub use versions::{
    check_freshness_requirements, validate_concrete_exact_version, validate_exact_version,
};

#[path = "catalog_rust_manifests.rs"]
mod rust_manifests;
pub use rust_manifests::{RUST_CHANNEL_MANIFESTS, rust_channel_manifest_sha256};

pub use velnor_actions_contract::config::jactionlint_verifier::{
    JACTIONLINT_DIGESTS, JactionlintDigest, jactionlint_archive_sha256,
    jactionlint_executable_sha256,
};

#[path = "catalog_tool.rs"]
mod tool;
pub use tool::PinnedTool;

#[path = "catalog_project.rs"]
pub mod project;
use versions::tool_source;

pub const MISE_VERSION: &str = "2026.10.20";
pub const MISE_BINARY_SHA256_LINUX_X64: &str =
    "2eb27467b6f369915ee76f18d66a0696cdb5c20e7ecc8cb49c2bfaca60f0fc9c";
pub const RUST_VERSION: &str = "1.99.1";
pub const MR_BOXINGTON_VERSION: &str = "1.28.0";
pub const GH_VERSION: &str = "2.103.3";
pub use velnor_actions_contract::config::jactionlint_verifier::JACTIONLINT_VERSION;
pub const SHELLCHECK_VERSION: &str = "0.11.0";
pub const NEXTEST_VERSION: &str = "0.9.150";
pub const OPENTOFU_VERSION: &str = "1.13.2";
pub const OPENTOFU_SHA256_LINUX_AMD64: &str =
    "ac05d9952662c7db28dd57e93f0a68018837bfdf9342384ec0987977c16a9645";
pub const OPENTOFU_SHA256_LINUX_ARM64: &str =
    "88e2d9e3f94f97b76ab2faf55ade87578f56aed19c5e45fa139d70e994366d1f";
pub const OPENTOFU_SHA256_DARWIN_AMD64: &str =
    "dc6e4eb1d3ed6550c2ef052596f933dbcd8c806957665ea83ae7537147cb3074";
pub const OPENTOFU_SHA256_DARWIN_ARM64: &str =
    "0d1cdc85f38992055b48e7fb161304a2ddfe2e5ee0ff68ffec9d0349c8bd1b59";
pub const RELEASE_PLZ_VERSION: &str = "0.4.1";
pub const CRANE_VERSION: &str = "0.22.1";
pub const BUN_VERSION: &str = "1.4.4";
pub const COSIGN_VERSION: &str = "3.1.4";
pub const COSIGN_LINUX_AMD64_BYTES: usize = 145_688_404;
pub const COSIGN_LINUX_AMD64_SHA256: &str =
    "424e2c97d0b45fd7a6b608a87f090f35633f4c23ece5f092d48bba11aaf4bbfa";
pub const COSIGN_LINUX_ARM64_BYTES: usize = 136_437_761;
pub const COSIGN_LINUX_ARM64_SHA256: &str =
    "ade83b7aac62e131cacfe96d958948b2150cc9efec46a570a2649b8e27dedad2";
pub const COSIGN_DARWIN_AMD64_BYTES: usize = 105_951_973;
pub const COSIGN_DARWIN_AMD64_SHA256: &str =
    "9d1e4abe2b6be279ed0d2ff706fde1ccd738747c834e3e6d44039c238867334f";
pub const COSIGN_DARWIN_ARM64_BYTES: usize = 99_023_586;
pub const COSIGN_DARWIN_ARM64_SHA256: &str =
    "9f9b8cd557cf2e5b3df3373a00baa005a86b02391e3e06dd0ce37b3a1533d3a4";

const NEXTEST_TOOL_SPEC_PREFIX: &str = "aqua:nextest-rs/nextest/cargo-nextest";

pub const RUST_TARGET_TRIPLE: &str = "x86_64-unknown-linux-gnu";

const TOOL_PLATFORMS: [&str; 2] = ["ubuntu-24.04", "ubuntu-26.04"];

const PLACEHOLDER_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCatalog {
    rust: String,
    mr_boxington: String,
    gh: String,
    jactionlint: String,
    shellcheck: String,
    nextest: String,
    opentofu: String,
    release_plz: String,
    crane: String,
    bun: String,
    project: std::collections::BTreeSet<PinnedTool>,
    emission: bool,
    export_rustup_toolchain: bool,
    rust_mr_boxington: bool,
}

impl ToolCatalog {
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            rust: RUST_VERSION.to_owned(),
            mr_boxington: MR_BOXINGTON_VERSION.to_owned(),
            gh: GH_VERSION.to_owned(),
            jactionlint: JACTIONLINT_VERSION.to_owned(),
            shellcheck: SHELLCHECK_VERSION.to_owned(),
            nextest: NEXTEST_VERSION.to_owned(),
            opentofu: OPENTOFU_VERSION.to_owned(),
            release_plz: RELEASE_PLZ_VERSION.to_owned(),
            crane: CRANE_VERSION.to_owned(),
            bun: BUN_VERSION.to_owned(),
            project: std::collections::BTreeSet::new(),
            emission: false,
            export_rustup_toolchain: true,
            rust_mr_boxington: false,
        }
    }

    pub fn new(
        rust: &str,
        mr_boxington: &str,
        gh: &str,
        jactionlint: &str,
        shellcheck: &str,
        nextest: &str,
        opentofu: &str,
    ) -> Result<Self, MiseError> {
        for (tool, version) in [
            (PinnedTool::Rust, rust),
            (PinnedTool::MrBoxington, mr_boxington),
            (PinnedTool::Gh, gh),
            (PinnedTool::Jactionlint, jactionlint),
            (PinnedTool::Shellcheck, shellcheck),
            (PinnedTool::Nextest, nextest),
            (PinnedTool::Opentofu, opentofu),
        ] {
            validate_concrete_exact_version(tool.tool_name(), version)?;
        }
        Ok(Self {
            rust: rust.to_owned(),
            mr_boxington: mr_boxington.to_owned(),
            gh: gh.to_owned(),
            jactionlint: jactionlint.to_owned(),
            shellcheck: shellcheck.to_owned(),
            nextest: nextest.to_owned(),
            opentofu: opentofu.to_owned(),
            release_plz: RELEASE_PLZ_VERSION.to_owned(),
            crane: CRANE_VERSION.to_owned(),
            bun: BUN_VERSION.to_owned(),
            project: std::collections::BTreeSet::new(),
            emission: false,
            export_rustup_toolchain: true,
            rust_mr_boxington: false,
        })
    }

    #[must_use]
    pub fn version(&self, tool: PinnedTool) -> &str {
        match tool {
            PinnedTool::Rust => &self.rust,
            PinnedTool::MrBoxington => &self.mr_boxington,
            PinnedTool::Gh => &self.gh,
            PinnedTool::Jactionlint => &self.jactionlint,
            PinnedTool::Shellcheck => &self.shellcheck,
            PinnedTool::Nextest => &self.nextest,
            PinnedTool::Opentofu => &self.opentofu,
            PinnedTool::ReleasePlz => &self.release_plz,
            PinnedTool::Crane => &self.crane,
            PinnedTool::Bun => &self.bun,
        }
    }

    #[must_use]
    pub fn tool_spec(&self, tool: PinnedTool) -> String {
        match tool {
            PinnedTool::Nextest => {
                format!("{NEXTEST_TOOL_SPEC_PREFIX}@{}", self.version(tool))
            }
            _ => format!("{}@{}", tool.tool_name(), self.version(tool)),
        }
    }

    #[must_use]
    pub fn rust_toolchain_name(&self) -> String {
        self.rust_toolchain_name_for_host(RUST_TARGET_TRIPLE)
    }

    #[must_use]
    pub fn rust_toolchain_name_for_host(&self, host: &str) -> String {
        format!("{}-{host}", self.version(PinnedTool::Rust))
    }

    #[must_use]
    pub fn tool_specs(&self, tools: &[PinnedTool]) -> Vec<String> {
        tools.iter().map(|tool| self.tool_spec(*tool)).collect()
    }

    #[must_use]
    pub fn rustup_toolchain(&self) -> String {
        self.version(PinnedTool::Rust).to_owned()
    }

    #[must_use]
    pub const fn with_export_rustup_toolchain(mut self, export: bool) -> Self {
        self.export_rustup_toolchain = export;
        self
    }

    #[must_use]
    pub const fn rust_mr_boxington(&self) -> bool {
        self.rust_mr_boxington
    }

    #[must_use]
    pub const fn with_rust_mr_boxington(mut self, flag: bool) -> Self {
        self.rust_mr_boxington = flag;
        self
    }

    #[must_use]
    pub const fn exports_rustup_toolchain(&self) -> bool {
        self.export_rustup_toolchain
    }

    #[must_use]
    pub fn rustup_toolchain_env(&self) -> Option<&str> {
        if self.export_rustup_toolchain {
            Some(&self.rust)
        } else {
            None
        }
    }
}
