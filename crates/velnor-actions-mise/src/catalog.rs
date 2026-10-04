//! Pinned tool catalog: exact mise tool selectors for every Velnor command.
//!
//! Qualified pins (native workloads rechecked 2026-10-03); project `mise.toml` selectors
//! never alter these pins.

/// Bootstrap lock and version-policy IO plus verification.
///
/// Hosted here because the crate root is frozen: `mise::catalog::lock` is
/// the canonical path for lock parsing and catalog-equality checks.
#[path = "lock.rs"]
pub mod lock;

#[path = "lock_verify.rs"]
mod lock_verify;

/// Pinned release-plz anonymous preparation integrity evidence.
#[path = "release_plz.rs"]
pub mod release_plz;

/// Closed MBX action source, distribution and comparison/output behavior authority.
#[path = "catalog_mbx_action_authority.rs"]
pub mod mbx_action_authority;

/// MBX provisioning modes (root frozen: `mise::catalog::mbx`).
#[path = "catalog_mbx.rs"]
pub mod mbx;
pub use mbx::MbxProvisioning;

/// Typed Rust installation options (pinned backend CLI grammar).
#[path = "catalog_rust_options.rs"]
pub mod rust_options;
pub use rust_options::RustInstallOptions;

/// Closed desktop compiler role and fixed independent pin.
#[path = "catalog_rust_desktop.rs"]
pub mod rust_desktop;
#[path = "catalog_rust_release.rs"]
mod rust_release;

/// Fixed isolated Rustup manager and proxy closure repair.
#[path = "catalog_rust_proxies.rs"]
pub mod rust_proxies;

/// Closed source-intent installer requiring pre-first-exec compiler authority.
#[path = "catalog_source_intent_install.rs"]
pub mod source_intent_install;

/// Adopted cold Foundation supplier authority, without compiler or SDK genesis grants.
#[path = "catalog_cold_foundation.rs"]
pub mod cold_foundation;
/// Pinned Rustup archive bootstrap, before Mise Rust installation.
#[path = "catalog_rust_bootstrap.rs"]
pub mod rust_bootstrap;
/// Official compiler component supply-chain authority, separate from current-run health.
#[path = "catalog_rust_compiler_authority.rs"]
pub mod rust_compiler_authority;
/// Sealed compiled manager expectations for closed Rust compiler roles.
#[path = "catalog_rustup_authority.rs"]
pub mod rustup_authority;

/// Pinned compiler integrity preflight and owner-side finalization.
#[path = "catalog_rust_health.rs"]
pub mod rust_health;

/// Measured official bootstrap assets for source building only.
#[path = "catalog_source_build_bootstrap.rs"]
pub mod source_build_bootstrap;

/// Reviewed owned source staging identities, separate from distribution qualification.
#[path = "catalog_owned_source.rs"]
pub mod owned_source;
/// Closed source and artifact qualification for generator-owned distributions.
#[path = "catalog_qualification.rs"]
pub mod qualification;

#[path = "catalog_selectors.rs"]
mod selectors;

/// Closed provider and engine profiles for native tool selection.
#[path = "catalog_native_profiles.rs"]
pub mod native_profiles;
pub use native_profiles::{JavaProvider, NativeToolProfile};

/// Closed consumer Gradle engine, wrapper bootstrap and Community Java context.
#[path = "catalog_gradle_consumer.rs"]
pub mod gradle_consumer;

/// Source-bound acquisition of an owned Mise distribution.
#[path = "catalog_mise_acquisition.rs"]
pub mod mise_acquisition;

/// Source-bound Rust preparation and exact install auditing.
#[path = "catalog_rust_prepare.rs"]
pub mod rust_prepare;

/// Source-only Root Rust qualification candidate; grants no installed SDK authority.
#[path = "catalog_root_rust_candidate.rs"]
pub mod root_rust_candidate;

/// Pure compiled tool installation and cache producer authority.
#[path = "catalog_tool_prepare.rs"]
pub mod tool_prepare;

/// Cold admission fallback for restored native executable trees.
#[path = "catalog_native_health.rs"]
pub mod native_health;

#[path = "catalog_preparation.rs"]
mod preparation;
pub use preparation::{
    delivery_tools, native_desktop, native_receipt_preparation, native_tool_context,
    native_validation, rust_cold,
};

/// Installed Java home materialization and persistent Gradle JVM authority.
#[path = "catalog_java_materialize.rs"]
pub mod java_materialize;

/// Exact-version validation plus qualification sources (split for size).
#[path = "catalog_versions.rs"]
mod versions;
pub use versions::{check_freshness_requirements, validate_exact_version};
#[path = "catalog_pins.rs"]
mod pins;
pub use pins::{
    ACTIONLINT_VERSION, GH_VERSION, MISE_VERSION, MR_BOXINGTON_VERSION, NEXTEST_VERSION,
    OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_SHA256_DARWIN_ARM64, OPENTOFU_SHA256_LINUX_AMD64,
    OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_VERSION, RELEASE_PLZ_VERSION, RUST_VERSION,
    SHELLCHECK_VERSION, ZIZMOR_VERSION,
};

/// Pinned Rust target triple: the single Linux target the runner fleet
/// maps to (`velnor_actions_contract::targets` stays the source of
/// truth; both runner labels resolve here).
pub const RUST_TARGET_TRIPLE: &str = "x86_64-unknown-linux-gnu";

/// Platforms every catalog tool supports (sorted, exact labels).
const TOOL_PLATFORMS: [&str; 2] = ["ubuntu-24.04", "ubuntu-26.04"];

/// Unqualified digest placeholder (ver §2): freshness replaces it per
/// upstream artifact sha256. Trusted validation rejects this value.
const PLACEHOLDER_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[path = "catalog_tools.rs"]
mod tools;
pub use tools::PinnedTool;

/// Exact API compatibility tool selection from the sole distribution registry.
pub const CARGO_SEMVER_CHECKS_VERSION: &str = qualification::CARGO_SEMVER_CHECKS_SELECTION_VERSION;

#[path = "catalog_workloads.rs"]
mod workloads;
pub use workloads::{
    ALINT_VERSION, BOLTFFI_VERSION, BUN_VERSION, CARGO_AUDIT_VERSION, CARGO_DENY_VERSION,
    GRADLE_VERSION, JAVA_VERSION, JQ_VERSION, NODE_MODULE_ABI_VERSION, NODE_NPM_VERSION,
    NODE_VERSION, PERIPHERY_VERSION, PYTHON_VERSION, REUSE_VERSION, RUBY_VERSION, SWIFT_VERSION,
    SWIFTLINT_VERSION, UV_VERSION, XCODEGEN_VERSION,
};

/// Consumer Gradle engine and independently qualified wrapper bootstrap bytes.
#[path = "catalog_gradle.rs"]
pub mod gradle;
pub use gradle::{
    GRADLE_WRAPPER_DISTRIBUTION_SHA256, GRADLE_WRAPPER_JAR_SHA256, GRADLE_WRAPPER_SCRIPT_SHA256,
    GRADLE_WRAPPER_VERSION, POSTGRES_FIXTURE_IMAGE,
};

/// Homebrew source and portable Ruby authority for native preparation.
#[path = "catalog_homebrew.rs"]
pub mod homebrew;

/// Native workload command requests.
#[path = "workload.rs"]
pub mod workload;

#[path = "catalog/tool_catalog.rs"]
mod tool_catalog;
pub use tool_catalog::ToolCatalog;
