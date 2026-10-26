// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use velnor_actions_contract::{
    config::NativeHomebrewOperation, package_release::PackageReleaseChannel,
};

const OPERATION_ENV: &str = "VELNOR_NATIVE_CHECK_OPERATION";

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeCheckResult {
    pub schema: u32,
    pub operation: NativeHomebrewOperation,
    pub identity: crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    pub outcome: NativeCheckOutcome,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum NativeCheckOutcome {
    TapTree {
        tree_digest: String,
    },
    FormulaContract {
        formula_bytes: Vec<u8>,
        public_release_receipt_digest: Option<String>,
    },
    Install {
        evidence: Box<NativeHomebrewInstallEvidence>,
        public_release_receipt_digest: String,
        hosted_profile: NativeHomebrewHostedProfileEvidence,
    },
    AdoptionContract {
        evidence: velnor_actions_contract::homebrew_adoption::HomebrewAdoptionEvidence,
    },
}

pub(crate) type NativeHomebrewInstallEvidence = crate::homebrew::install::HomebrewInstallEvidence;

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeHomebrewHostedProfileEvidence {
    pub source_commit: String,
    pub source_tree: String,
    pub source_describe: String,
    pub source_tag_commit: String,
    pub portable_ruby_version: String,
    pub portable_ruby_archive_sha256: String,
    pub portable_ruby_binary_sha256: String,
    pub active_prefix_device: u64,
    pub active_prefix_inode: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct VerifiedNativeHomebrewInstall {
    run_key: String,
    head: String,
    check_id: String,
    operation: NativeHomebrewOperation,
    channel: PackageReleaseChannel,
    platform: velnor_actions_contract::CheckPlatform,
    task_id: String,
    plan_id: String,
    entry_digest: String,
    task_digest: String,
    input_digest: String,
    closure_digest: String,
    tap_repository: String,
    tap_repository_id: u64,
    tap_base_sha: String,
    formula_path: String,
    formula_blob_sha: String,
}

impl VerifiedNativeHomebrewInstall {
    pub(crate) fn operation(&self) -> NativeHomebrewOperation {
        self.operation
    }
    pub(crate) fn channel(&self) -> PackageReleaseChannel {
        self.channel
    }
    pub(crate) fn platform(&self) -> velnor_actions_contract::CheckPlatform {
        self.platform
    }
    pub(crate) fn run_key(&self) -> &str {
        &self.run_key
    }
    pub(crate) fn head(&self) -> &str {
        &self.head
    }
    pub(crate) fn check_id(&self) -> &str {
        &self.check_id
    }
    pub(crate) fn task_id(&self) -> &str {
        &self.task_id
    }
    pub(crate) fn plan_id(&self) -> &str {
        &self.plan_id
    }
    pub(crate) fn entry_digest(&self) -> &str {
        &self.entry_digest
    }
    pub(crate) fn task_digest(&self) -> &str {
        &self.task_digest
    }
    pub(crate) fn input_digest(&self) -> &str {
        &self.input_digest
    }
    pub(crate) fn closure_digest(&self) -> &str {
        &self.closure_digest
    }
    pub(crate) fn tap_repository(&self) -> &str {
        &self.tap_repository
    }
    pub(crate) fn tap_repository_id(&self) -> u64 {
        self.tap_repository_id
    }
    pub(crate) fn tap_base_sha(&self) -> &str {
        &self.tap_base_sha
    }
    pub(crate) fn formula_path(&self) -> &str {
        &self.formula_path
    }
    pub(crate) fn formula_blob_sha(&self) -> &str {
        &self.formula_blob_sha
    }
}

#[path = "native_check_runtime/execute.rs"]
mod execute;
#[path = "native_check_runtime/formula.rs"]
mod formula;
#[path = "native_check_runtime/install.rs"]
mod install;

pub(crate) use execute::execute_native_homebrew_check_to;
pub(crate) use formula::{render_current_preview_formula, verify_formula};

#[path = "native_check_runtime/entry.rs"]
mod entry;

pub use entry::execute_native_homebrew_check_from_env;
