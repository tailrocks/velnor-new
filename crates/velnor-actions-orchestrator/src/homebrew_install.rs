//! Homebrew installation coordination and archived-keg verification.
//!
//! This module owns filesystem writes, inventory validation, and install
//! evidence. All child processes are constructed by Mise's closed adapter.

use std::fs::File;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::homebrew_spec::{
    HomebrewAuxiliaryTest, HomebrewFormulaReleaseLiterals, HomebrewFormulaSpec,
    HomebrewInstalledMode, ValidatedHomebrewProjection,
};
use velnor_actions_mise::CheckDeadline;

#[path = "homebrew_install_coordinator.rs"]
mod coordinator;
#[path = "homebrew_install_fresh.rs"]
mod fresh;
#[path = "homebrew_install_inspect.rs"]
mod inspect;
#[path = "homebrew_install_inventory.rs"]
mod inventory;
#[path = "homebrew_install_request.rs"]
mod request;
#[path = "homebrew_install_support.rs"]
mod support;
#[path = "homebrew_install_tap.rs"]
mod tap;
pub(crate) use coordinator::upgrade_homebrew_candidate;
pub(crate) use fresh::{install_homebrew_candidate, install_homebrew_candidate_with_custody};
pub(crate) use support::elf_facts_from_bytes;

/// Qualified Homebrew executable for the initial hosted ARM Mac profile.
pub(crate) const HOMEBREW_EXECUTABLE: &str = "/opt/homebrew/bin/brew";
/// Exact Homebrew source identity admitted by the hosted profile.
pub(crate) const HOMEBREW_VERSION: &str = "Homebrew 7.0.10-84-g7a056ff";
/// Fixed Homebrew prefix for the initial hosted ARM Mac profile.
pub(crate) const HOMEBREW_PREFIX: &str = "/opt/homebrew";
const HOMEBREW_CELLAR: &str = "/opt/homebrew/Cellar";
/// Qualified Homebrew Cleaner normalizes installed host executables to `0555`,
/// preserving execute access and removing owner write permission from `0755`.
pub(crate) const HOMEBREW_HOST_INSTALLED_MODE: u32 = 0o555;
/// Stable local tap identity used for candidate formula receipts.
pub(super) const HOMEBREW_CANDIDATE_TAP: &str = "velnor/native-install";
/// Physical formula directory consumed by the pinned Homebrew binary.
pub(super) const HOMEBREW_CANDIDATE_FORMULAE: &str =
    "/opt/homebrew/Library/Taps/velnor/homebrew-native-install/Formula";
const CAPTURE_LIMIT: usize = 8 * 1024 * 1024;
const FORMULA_LIMIT: usize = 1024 * 1024;
const FILE_LIMIT: u64 = 512 * 1024 * 1024;

/// Expected bytes and placement for one authenticated payload member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HomebrewInstallFile {
    /// Exact authenticated release asset carrying this member.
    pub(crate) asset: String,
    /// Exact member name in that asset.
    pub(crate) member: String,
    /// Relative path under the installed keg.
    pub(crate) destination: PathBuf,
    /// SHA-256 of the authenticated extracted member bytes.
    pub(crate) sha256: String,
    /// Required installed mode, without file-type bits.
    pub(crate) mode: u32,
    /// Closed offline post-install check selected by the approved spec.
    pub(crate) test: HomebrewAuxiliaryTest,
    /// Expected ELF facts for a `linux_elf` member; absent for host files.
    pub(crate) elf: Option<ElfFacts>,
}

/// ELF header facts proven from the authenticated Linux payload member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ElfFacts {
    /// ELF class byte (`1` for 32-bit, `2` for 64-bit).
    pub(crate) class: u8,
    /// ELF data byte (`1` little-endian, `2` big-endian).
    pub(crate) endian: u8,
    /// ELF machine identifier.
    pub(crate) machine: u16,
}

/// Stable payload mode for non-host auxiliary files in `libexec`.
pub(crate) const fn auxiliary_installed_mode(mode: HomebrewInstalledMode) -> u32 {
    match mode {
        HomebrewInstalledMode::Data => 0o644,
        HomebrewInstalledMode::Executable => 0o755,
    }
}

/// Caller-owned paths used only for this isolated hosted install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HomebrewInstalledFileEvidence {
    /// Authenticated archive containing the installed bytes.
    pub(crate) asset: String,
    /// Exact authenticated archive member name.
    pub(crate) member: String,
    /// Exact relative installed keg path.
    pub(crate) destination: PathBuf,
    /// SHA-256 of the observed installed bytes.
    pub(crate) sha256: String,
    /// Observed installed permission bits.
    pub(crate) mode: u32,
    /// Fixed offline content check applied to this file.
    pub(crate) test: HomebrewAuxiliaryTest,
    /// Observed ELF facts when the file is a Linux executable.
    pub(crate) elf: Option<ElfFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Isolated paths Homebrew may use during one candidate installation.
pub(crate) struct HomebrewInstallPaths {
    /// Private runner home used by Homebrew.
    pub(crate) home: PathBuf,
    /// Private runner temporary directory used by Homebrew.
    pub(crate) temp: PathBuf,
    /// Private directory holding the candidate formula.
    pub(crate) staging: PathBuf,
}

/// Typed install request; it contains no argv, executable path, or credential.
///
/// The request itself is not a release-authentication proof. Its caller must
/// derive every payload digest from descriptor-verified custody, invoke this
/// adapter only after release verification, and hold custody through return.
#[derive(Debug)]
pub(crate) struct HomebrewInstallRequest<'a> {
    /// Approved generic formula policy.
    pub(crate) spec: &'a HomebrewFormulaSpec,
    /// Private validated projection against the authenticated release plan.
    pub(crate) projection: &'a ValidatedHomebrewProjection,
    /// Release literals validated against this projection.
    pub(crate) literals: &'a HomebrewFormulaReleaseLiterals,
    /// Exact generated formula bytes.
    pub(crate) formula_bytes: &'a [u8],
    /// Expected SHA-256 of those exact bytes.
    pub(crate) formula_sha256: &'a str,
    /// Complete selected host and auxiliary installed-file inventory.
    pub(crate) files: &'a [HomebrewInstallFile],
    /// Owned private runner paths.
    pub(crate) paths: &'a HomebrewInstallPaths,
    /// Existing absolute deadline shared with the native check.
    pub(crate) deadline: CheckDeadline,
}

/// Evidence produced by one successful install, archived recipe test, and recheck.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HomebrewInstallEvidence {
    /// Approved formula name.
    pub(crate) formula: String,
    /// Exact release version in the rendered formula.
    pub(crate) version: String,
    /// Qualified Homebrew source identity.
    pub(crate) brew_version: String,
    /// Qualified Homebrew prefix.
    pub(crate) prefix: String,
    /// Exact Cellar path containing the new keg.
    pub(crate) cellar: String,
    /// Exact newly created keg path.
    pub(crate) keg: String,
    /// Unix install timestamp recorded by this exact Homebrew receipt.
    pub(crate) install_time: u64,
    /// SHA-256 of the native Homebrew install receipt bytes.
    pub(crate) receipt_sha256: String,
    /// SHA-256 of the archived formula recipe.
    pub(crate) recipe_sha256: String,
    /// SHA-256 of the installed payload inventory in canonical order.
    pub(crate) payload_sha256: String,
    /// Per-member installed archive bindings used to compute `payload_sha256`.
    pub(crate) installed_files: Vec<HomebrewInstalledFileEvidence>,
    /// SHA-256 over the fixed `--version` output of every installed host binary.
    pub(crate) version_output_sha256: String,
    /// Final digest over the complete adapter evidence.
    pub(crate) verification_sha256: String,
}

/// Private custody of a keg installed by this adapter during the same run.
/// The open directory descriptor prevents inode reuse while a later candidate
/// is upgraded and the original keg is rechecked.
#[derive(Debug)]
pub(crate) struct HomebrewInstalledKegCustody {
    keg: PathBuf,
    state: KegState,
    directory: File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    device: u64,
    inode: u64,
    mode: u32,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KegState {
    receipt: Vec<u8>,
    receipt_sha256: String,
    sbom_sha256: String,
    sbom: Vec<u8>,
    recipe_sha256: String,
    payload_sha256: String,
    installed_files: Vec<HomebrewInstalledFileEvidence>,
    version_output_sha256: String,
    install_time: u64,
    keg_identity: Snapshot,
}
