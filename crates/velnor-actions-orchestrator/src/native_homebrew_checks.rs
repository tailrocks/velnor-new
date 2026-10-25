//! Closed native Homebrew check identities and plan entries.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::config::{NativeHomebrewCheck, NativeHomebrewOperation};
use velnor_actions_contract::package_release::PackageReleaseChannel;
use velnor_actions_contract::{
    CheckPlatform, ExecuteTaskIds, ExecuteTaskRef, MatrixEntry, ObligationDecision, PlanGenerator,
    PlanObligation, PlannedPlatform, ProposedTask, ResourceClass, ResourceDemand, Stack,
    StackExtension, TaskConfiguration, TaskGenerator, TaskIdentity, TaskInput, VcsInputs,
    WorkflowEvent, canonical_json_bytes, digest_b3, input_digest,
};
use velnor_actions_mise::CheckDeadline;

#[path = "native_homebrew_check_inventory.rs"]
mod inventory;
#[path = "native_homebrew_check_plan.rs"]
mod plan;
#[path = "native_homebrew_plan_envelope.rs"]
mod plan_envelope;
#[path = "native_homebrew_check_required.rs"]
mod required;
#[path = "native_homebrew_check_seed.rs"]
mod seed;
#[path = "native_homebrew_check_tree.rs"]
mod tree;

/// Distinct protected authority custody for ordinary checks and the one-time seed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum NativeCheckAuthorityIdentity {
    /// Source policy admitted through the protected tap-base locator.
    Source {
        source_repository_id: u64,
        authority_sha: String,
        source_plan_digest: String,
        locator_blob_sha: String,
        formula_spec_digest: String,
        cutover: Option<Box<crate::homebrew::structural_cutover::HomebrewSeedOutputBaseIdentity>>,
    },
    /// Exact protected bootstrap transition, admitted only at its pinned tap base.
    Seed {
        transition_id: String,
        transition_digest: String,
        base_tree: String,
        sdk_source_sha: String,
        sdk_version: String,
        sdk_manifest_sha256: String,
        sdk_seed_runner_binary_sha256: String,
    },
}

/// Native Homebrew check identity extension schema.
pub(crate) const IDENTITY_SCHEMA: &str = "homebrew-native-check-identity-v2";
/// Closed execution operation version.
pub(crate) const OPERATION_VERSION: &str = "native-homebrew-check-v1";
/// Dispatch operation accepted by the staged SDK.
pub(crate) const EXECUTE_OPERATION: &str = "execute-native-homebrew-check-v1";

/// Plan-bound identity carried in matrix metadata and the task digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeCheckIdentityExtension {
    pub schema: String,
    pub check_id: String,
    pub operation: NativeHomebrewOperation,
    pub formula_channel: Option<velnor_actions_contract::package_release::PackageReleaseChannel>,
    pub operation_version: String,
    pub sdk_version: String,
    pub sdk_sha256: String,
    pub authority: NativeCheckAuthorityIdentity,
    pub tap_repository: String,
    pub tap_repository_id: u64,
    pub tap_base_sha: String,
    pub head: String,
    pub tree_object: String,
    pub formula_path: Option<String>,
    pub formula_blob_sha: Option<String>,
    pub runner: String,
    pub platform: CheckPlatform,
    pub tool_profile: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SeedTransitionAdmission {
    pub(crate) transition: crate::homebrew::adoption::ProtectedHomebrewAdoptionTransition,
    pub(crate) evidence: velnor_actions_contract::homebrew_adoption::HomebrewAdoptionEvidence,
    pub(crate) sdk: crate::homebrew::sdk_binding::PublishedSdkBindingIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeExpectedInstallFile {
    pub asset: String,
    pub member: String,
    pub destination: PathBuf,
    pub mode: u32,
    pub test: velnor_actions_contract::homebrew_spec::HomebrewAuxiliaryTest,
    pub elf: Option<(u8, u8, u16)>,
}

pub(crate) use inventory::expected_install_files;
pub(crate) use plan::{derive, proposal};
pub(crate) use required::{required_native_homebrew_checks, validate_required_plan};
pub(crate) use seed::validate_seed_transition;
pub(crate) use tree::{caller_repository, pull_request_number_from_env, read_checked_tap_tree};

#[cfg(test)]
use required::{seed_check_metadata_matches, seed_required_operation_present};
#[cfg(test)]
use seed::seed_pull_context_matches;
#[cfg(test)]
use tree::caller_repository_matches;

#[cfg(test)]
#[path = "native_homebrew_check_tests.rs"]
mod tests;
