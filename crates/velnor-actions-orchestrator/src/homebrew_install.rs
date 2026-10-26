// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

pub(crate) const HOMEBREW_EXECUTABLE: &str = "/opt/homebrew/bin/brew";
pub(crate) const HOMEBREW_VERSION: &str = "Homebrew 7.0.10-84-g7a056ff";
pub(crate) const HOMEBREW_PREFIX: &str = "/opt/homebrew";
const HOMEBREW_CELLAR: &str = "/opt/homebrew/Cellar";
pub(crate) const HOMEBREW_HOST_INSTALLED_MODE: u32 = 0o555;
pub(super) const HOMEBREW_CANDIDATE_TAP: &str = "velnor/native-install";
pub(super) const HOMEBREW_CANDIDATE_FORMULAE: &str =
    "/opt/homebrew/Library/Taps/velnor/homebrew-native-install/Formula";
const CAPTURE_LIMIT: usize = 8 * 1024 * 1024;
const FORMULA_LIMIT: usize = 1024 * 1024;
const FILE_LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HomebrewInstallFile {
    pub(crate) asset: String,
    pub(crate) member: String,
    pub(crate) destination: PathBuf,
    pub(crate) sha256: String,
    pub(crate) mode: u32,
    pub(crate) test: HomebrewAuxiliaryTest,
    pub(crate) elf: Option<ElfFacts>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ElfFacts {
    pub(crate) class: u8,
    pub(crate) endian: u8,
    pub(crate) machine: u16,
}

pub(crate) const fn auxiliary_installed_mode(mode: HomebrewInstalledMode) -> u32 {
    match mode {
        HomebrewInstalledMode::Data => 0o644,
        HomebrewInstalledMode::Executable => 0o755,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HomebrewInstalledFileEvidence {
    pub(crate) asset: String,
    pub(crate) member: String,
    pub(crate) destination: PathBuf,
    pub(crate) sha256: String,
    pub(crate) mode: u32,
    pub(crate) test: HomebrewAuxiliaryTest,
    pub(crate) elf: Option<ElfFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HomebrewInstallPaths {
    pub(crate) home: PathBuf,
    pub(crate) temp: PathBuf,
    pub(crate) staging: PathBuf,
}

#[derive(Debug)]
pub(crate) struct HomebrewInstallRequest<'a> {
    pub(crate) spec: &'a HomebrewFormulaSpec,
    pub(crate) projection: &'a ValidatedHomebrewProjection,
    pub(crate) literals: &'a HomebrewFormulaReleaseLiterals,
    pub(crate) formula_bytes: &'a [u8],
    pub(crate) formula_sha256: &'a str,
    pub(crate) files: &'a [HomebrewInstallFile],
    pub(crate) paths: &'a HomebrewInstallPaths,
    pub(crate) deadline: CheckDeadline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HomebrewInstallEvidence {
    pub(crate) formula: String,
    pub(crate) version: String,
    pub(crate) brew_version: String,
    pub(crate) prefix: String,
    pub(crate) cellar: String,
    pub(crate) keg: String,
    pub(crate) install_time: u64,
    pub(crate) receipt_sha256: String,
    pub(crate) recipe_sha256: String,
    pub(crate) payload_sha256: String,
    pub(crate) installed_files: Vec<HomebrewInstalledFileEvidence>,
    pub(crate) version_output_sha256: String,
    pub(crate) verification_sha256: String,
}

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
