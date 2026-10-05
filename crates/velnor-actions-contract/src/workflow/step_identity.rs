//! Typed workflow step identity and semantic role.

use super::step::{Step, StepKind};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Fixed local action path used for exact provider-cache admission.
pub const TOFU_PROVIDER_ADMISSION_USES: &str = "./.github/actions/tofu-provider-admission";

/// Stable GitHub Actions id for a step whose outputs have consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepId {
    /// Planner output consumed by downstream jobs.
    Plan,
    /// Baseline publisher output consumed by the release uploader.
    PublishBaseline,
    /// MBX cache identity outputs consumed by the bundle restore.
    MbxBundleKey,
    /// MBX action cache restore outputs exposed to later workflow steps.
    MbxCacheRestore,
    /// MBX bundle restore output consumed by its import step.
    MbxBundle,
    /// MBX export output consumed by its save gate.
    MbxExport,
    /// `OpenTofu` provider-cache restore outputs consumed by admission.
    TofuProviders,
}

impl StepId {
    /// Fixed YAML spelling for this step output owner.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::PublishBaseline => "publish-baseline",
            Self::MbxBundleKey => "mbx-cache-key",
            Self::MbxCacheRestore => "mbx",
            Self::MbxBundle => "mbx-bundle",
            Self::MbxExport => "mbx-export",
            Self::TofuProviders => "tofu-providers",
        }
    }
}

/// Semantic step role used for workflow composition and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepRole {
    /// Checkout whose credentials are disabled.
    Checkout,
    /// Install the exact catalog tools used by later steps.
    PreparePinnedTools,
    /// Install the pinned Rust components used by task work.
    PrepareRustComponents,
    /// Pinned Mise action that owns its built-in tools cache.
    MiseSetup,
    /// Authenticated cargo-deny analyzer bootstrap.
    CargoDeny,
    /// Authenticated cargo-machete analyzer bootstrap.
    CargoMachete,
    /// Authenticated actionlint analyzer bootstrap.
    Actionlint,
    /// Authenticated zizmor analyzer bootstrap.
    Zizmor,
    /// Planner producing the plan outputs.
    PlanProducer,
    /// Baseline publisher producing the artifact name output.
    BaselinePublisher,
    /// Digest-verified Velnor executable acquisition.
    AcquireVelnor,
    /// Pre-seed copy of the helper into runner-temp staging.
    PreseedStage,
    /// Download of the plan artifact for final merging.
    DownloadPlan,
    /// Generated-tree freshness check.
    CheckGenerated,
    /// Upload of the planner report artifact.
    PublishPlan,
    /// Upload of one matrix task report.
    MatrixReportUpload,
    /// Download of a candidate attestation artifact.
    AttestationDownload,
    /// Fetch of exact matrix report artifacts.
    FetchReports,
    /// Upload of the final merged report.
    PublishFinal,
    /// `OpenTofu`/Rust plan formatting step.
    PlanFormat,
    /// Per-crate minimum-supported-Rust-version qualification.
    MsrvQualification,
    /// One elected tools-cache writer.
    ToolsCacheSave,
    /// Restore of the shared Cargo registry and git sources snapshot.
    CargoSourcesRestore,
    /// Single elected writer for the shared Cargo sources snapshot.
    CargoSourcesSave,
    /// Fetch probe used to populate the Cargo source snapshot on a miss.
    CargoSourcesFetch,
    /// Cargo-only registry cache restore.
    CargoRegistryRestore,
    /// One elected `OpenTofu` provider-cache writer.
    TofuProvidersSave,
    /// `OpenTofu` provider-cache restore whose output is admitted before use.
    TofuProvidersRestore,
    /// `OpenTofu` provider-cache admission before any provider consumer.
    TofuProvidersAdmission,
    /// Pre-seed build of the candidate helper.
    PreseedBuild,
    /// Pre-seed manifest creation.
    PreseedManifest,
    /// Verification of the freshly built pre-seed candidate.
    PreseedVerifyBuild,
    /// Pre-seed candidate artifact upload.
    PreseedUpload,
    /// Pre-seed candidate artifact download.
    PreseedDownload,
    /// Pre-seed manifest verification.
    PreseedVerifyManifest,
    /// MBX provider-cache import from its bundle.
    MbxBundleImport,
    /// MBX bundle export from the live object store.
    MbxBundleExport,
    /// MBX single-bundle cache save.
    MbxBundleSave,
    /// MBX action cache restore.
    MbxCache,
    /// MBX local backend setup for Scale Set jobs.
    MbxLocalSetup,
    /// Exact MBX and Rust toolchain preflight.
    MbxPreflight,
    /// MBX cache-primary-key prefix producer.
    MbxBundleKey,
    /// MBX bundle cache restore.
    MbxBundleRestore,
}

impl StepRole {
    /// Check role-specific payload kind and identity.
    pub(crate) fn validate(self, kind: &StepKind, job: &str) -> Result<(), ContractError> {
        if self.matches_payload(kind) {
            Ok(())
        } else {
            Err(ContractError::identity(
                "step.role",
                format!("role_kind_mismatch:{job}:{self:?}"),
            ))
        }
    }

    /// Match one role against its fixed kind and payload family.
    fn matches_payload(self, kind: &StepKind) -> bool {
        match self {
            Self::Checkout => matches!(kind, StepKind::Action { uses, with, .. }
                if action_has_prefix(uses, "actions/checkout@")
                    && with.get("persist-credentials").is_some_and(|value| value == "false")),
            Self::MiseSetup => valid_mise_setup(kind),
            Self::TofuProvidersAdmission => valid_tofu_admission(kind),
            Self::PlanProducer => internal_operation(kind, "plan-v1"),
            Self::BaselinePublisher => internal_operation(kind, "publish-baseline-v1"),
            Self::FetchReports => internal_operation(kind, "fetch-reports-v1"),
            Self::DownloadPlan | Self::AttestationDownload | Self::PreseedDownload => {
                action_has_prefix_for_kind(kind, "actions/download-artifact@")
            }
            Self::PublishPlan
            | Self::MatrixReportUpload
            | Self::PublishFinal
            | Self::PreseedUpload => action_has_prefix_for_kind(kind, "actions/upload-artifact@"),
            Self::ToolsCacheSave
            | Self::TofuProvidersSave
            | Self::MbxBundleSave
            | Self::CargoSourcesSave => action_has_prefix_for_kind(kind, "actions/cache/save@"),
            Self::CargoSourcesRestore | Self::MbxBundleRestore => {
                action_has_prefix_for_kind(kind, "actions/cache/restore@")
            }
            Self::TofuProvidersRestore => matches!(kind, StepKind::Action { uses, with, .. }
                if action_has_prefix(uses, "actions/cache/restore@")
                    && with.get("restore-keys").is_some_and(String::is_empty)),
            Self::CargoRegistryRestore => action_has_prefix_for_kind(kind, "Swatinem/rust-cache@"),
            Self::MbxCache => valid_mbx_cache(kind, false),
            Self::MbxLocalSetup => valid_mbx_cache(kind, true),
            Self::PreparePinnedTools
            | Self::PrepareRustComponents
            | Self::CargoDeny
            | Self::CargoMachete
            | Self::Actionlint
            | Self::Zizmor
            | Self::AcquireVelnor
            | Self::PreseedStage
            | Self::CheckGenerated
            | Self::PlanFormat
            | Self::MsrvQualification
            | Self::CargoSourcesFetch
            | Self::PreseedBuild
            | Self::PreseedManifest
            | Self::PreseedVerifyBuild
            | Self::PreseedVerifyManifest
            | Self::MbxBundleExport
            | Self::MbxBundleImport
            | Self::MbxBundleKey
            | Self::MbxPreflight => matches!(kind, StepKind::Shell { .. }),
        }
    }

    /// Output identity required for a role that publishes step outputs.
    pub(crate) const fn required_id(self) -> Option<StepId> {
        match self {
            Self::PlanProducer => Some(StepId::Plan),
            Self::BaselinePublisher => Some(StepId::PublishBaseline),
            Self::MbxBundleKey => Some(StepId::MbxBundleKey),
            Self::MbxCache => Some(StepId::MbxCacheRestore),
            Self::MbxBundleRestore => Some(StepId::MbxBundle),
            Self::MbxBundleExport => Some(StepId::MbxExport),
            Self::TofuProvidersRestore => Some(StepId::TofuProviders),
            _ => None,
        }
    }

    /// Role required for a typed step output identity.
    pub(crate) const fn for_id(id: StepId) -> Self {
        match id {
            StepId::Plan => Self::PlanProducer,
            StepId::PublishBaseline => Self::BaselinePublisher,
            StepId::MbxBundleKey => Self::MbxBundleKey,
            StepId::MbxCacheRestore => Self::MbxCache,
            StepId::MbxBundle => Self::MbxBundleRestore,
            StepId::MbxExport => Self::MbxBundleExport,
            StepId::TofuProviders => Self::TofuProvidersRestore,
        }
    }
}

/// True when an action payload uses a fixed name prefix.
fn action_has_prefix_for_kind(kind: &StepKind, prefix: &str) -> bool {
    matches!(kind, StepKind::Action { uses, .. } if action_has_prefix(uses, prefix))
}

/// True when a pinned action ref starts with its required owner/repository.
fn action_has_prefix(uses: &str, prefix: &str) -> bool {
    uses.starts_with(prefix)
}

/// True when an internal operation matches its role contract.
fn internal_operation(kind: &StepKind, expected: &str) -> bool {
    matches!(kind, StepKind::Internal { operation } if operation == expected)
}

/// Validate the typed Mise setup payload shape shared by render paths.
fn valid_mise_setup(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
        if action_has_prefix(uses, "jdx/mise-action@")
            && with.get("version").is_some_and(|value| !value.is_empty())
            && with.get("sha256").is_some_and(|value| !value.is_empty())
            && with.get("install").is_some_and(|value| value == "false")
            && with.get("env").is_some_and(|value| value == "false")
            && with.get("cache").is_some_and(|value| value == "false" || value == "true")
            && with.get("cache_save").is_some_and(|value| value == "false" || value == "true"))
}

/// Validate the local provider admission call and exact same-restore outputs.
fn valid_tofu_admission(kind: &StepKind) -> bool {
    matches!(kind, StepKind::Action { uses, with, env }
    if uses == TOFU_PROVIDER_ADMISSION_USES
        && env.is_empty()
        && with.len() == 4
        && with.get("cache-hit").is_some_and(|value| value == "${{ steps.tofu-providers.outputs.cache-hit }}")
        && with.get("matched-key").is_some_and(|value| value == "${{ steps.tofu-providers.outputs.cache-matched-key }}")
        && with.get("expected-key").is_some_and(|value| !value.trim().is_empty())
        && with.get("cache-slug").is_some_and(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        }))
}

/// Validate MBX action backend mode for hosted cache and local setup roles.
fn valid_mbx_cache(kind: &StepKind, local: bool) -> bool {
    matches!(kind, StepKind::Action { uses, with, .. }
    if action_has_prefix(uses, "jdx/mr-boxington-action@")
        && if local {
            with.get("backend").is_some_and(|backend| backend == "local")
        } else {
            with.get("backend").is_none_or(|backend| backend == "github")
        })
}
/// Validate one serialized step scope, including role payloads and unique output IDs.
///
/// Workflow jobs and composite actions use separate output-ID scopes, so renderers
/// call this after internal expansion for each final serialized sequence.
/// # Errors
///
/// Returns a contract error for an invalid role/payload or duplicate step ID.
pub fn validate_step_sequence(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    let mut ids = BTreeSet::new();
    for step in steps {
        step.validate(scope)?;
        if let Some(step_id) = step.id
            && !ids.insert(step_id)
        {
            return Err(ContractError::identity(
                "job.steps.id",
                format!("duplicate_step_id:{scope}:{}", step_id.as_str()),
            ));
        }
    }
    Ok(())
}
