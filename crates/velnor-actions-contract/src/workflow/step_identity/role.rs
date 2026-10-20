use super::super::step::StepKind;
use super::super::step_id::StepId;
use super::payload::{
    action_has_prefix_for_kind, internal_operation, valid_cache_tar_launcher,
    valid_checkout_payload, valid_mbx_cache, valid_mise_setup, valid_tool_seed_payload,
    valid_tools_cache_identity,
};
use super::{
    BUN_REGISTRY_INSTALL_OPERATION, NORMAL_TOOLS_CACHE_EXPORT_OPERATION,
    NORMAL_TOOLS_CACHE_IMPORT_OPERATION, NORMAL_TOOLS_CACHE_STAGE_OPERATION,
    PRIVATE_FETCH_OPERATION, PRIVATE_REVOKE_OPERATION, PRIVATE_TOKEN_MINT_ACTION_PREFIX,
    TOOLS_CACHE_RESTORE_USES,
};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Semantic step role used for workflow composition and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepRole {
    /// Checkout whose credentials are disabled.
    Checkout,
    /// Copy from the trusted, exact-key host tool seed.
    ToolSeed,
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
    /// Authenticated markdownlint analyzer bootstrap.
    Markdownlint,
    /// Authenticated strict-JSON check bootstrap.
    StrictJson,
    /// Authenticated frontmatter-ID check bootstrap.
    FrontmatterId,
    /// Authenticated link-check analyzer bootstrap.
    LinkCheck,
    /// Authenticated native-validators probe bootstrap.
    NativeValidators,
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
    /// Installer producing PATH output for cache-action steps only.
    CacheTarLaunchers,
    /// Runtime identity action that gates the V2 tools-cache layer.
    ToolsCacheIdentity,
    /// Read-only restore wrapper for the V2 tools-cache payload.
    ToolsCacheRestore,
    /// CLI operation that computes the normal tools-cache staging path.
    NormalToolsCacheStage,
    /// CLI operation that checks and imports the normal tools-cache payload.
    NormalToolsCacheImport,
    /// CLI operation that checks and stages the normal tools-cache payload.
    NormalToolsCacheExport,
    /// Restore of the shared Cargo registry and git sources snapshot.
    CargoSourcesRestore,
    /// Single elected writer for the shared Cargo sources snapshot.
    CargoSourcesSave,
    /// Fetch probe used to populate the Cargo source snapshot on a miss.
    CargoSourcesFetch,
    /// Compute runtime cache trust and keys before cache restores.
    CacheKeys,
    /// One elected `OpenTofu` provider-cache writer.
    TofuProvidersSave,
    /// `OpenTofu` provider-cache restore/admission composite before provider use.
    TofuProvidersRestore,
    /// `OpenTofu` init/validate shell that consumes the admitted plugin cache.
    TofuProviderUse,
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
    /// Native MBX action owns hosted and local object-cache lifecycle.
    MbxCache,
    /// Exact MBX binary and object-store verification after the native action.
    MbxVersionCheck,
    /// Exact MBX and Rust toolchain preflight.
    MbxPreflight,
    /// Final workspace-target cleanup before the native MBX action post step.
    MbxWorkspaceCleanup,
    /// Mint of the read-only App installation token for private sources (HD-7).
    PrivateTokenMint,
    /// Velnor-owned fetch of private sources; the only step that may bind a
    /// private-source credential besides the revoke step (HD-7).
    PrivateFetch,
    /// Revocation of the private-source token, run `always()` (HD-7).
    PrivateRevoke,
    /// Producer of the measured qualification runtime identity file.
    QualificationRuntimeIdentity,
    /// Producer of bounded cache-root observations for one owner and layer.
    QualificationCacheRoots,
    /// Bun registry install using the workflow-scoped npm token.
    BunRegistryInstall,
    /// Hash-verified Playwright browser install; holds no credential.
    PlaywrightInstall,
    /// Binding of one emitted job owner and cache layer.
    QualificationCacheBind,
    /// Restore action for one emitted job owner and cache layer.
    QualificationCacheRestore,
    /// Import operation for one plan lane and cache layer.
    QualificationCacheImport,
    /// Read-only backend observation for one cache operation stage.
    QualificationCacheObservation,
    /// Export operation for one emitted job owner and cache layer.
    QualificationCacheExport,
    /// Save action for one emitted job owner and cache layer.
    QualificationCacheSave,
    /// Record the cache receipt produced by the required job.
    /// Qualification cache receipt writer in the required aggregation job.
    QualificationCacheReceipt,
    /// Upload of the completed qualification cache receipt artifact.
    QualificationCacheReceiptUpload,
    /// Runtime identity measurement after cache import.
    QualificationCacheRuntimeIdentityAfterImport,
    /// Backend observation for restore or save lifecycle.
    QualificationCacheCensus,
    /// Import of one owner-layer cache payload.
    QualificationCachePayloadImport,
    /// Export of one owner-layer cache payload.
    QualificationCachePayloadExport,
    /// Immutable evidence for one owner-layer lifecycle.
    QualificationCacheOwnerLayerEvidence,
    /// Aggregate owner evidence before job artifact upload.
    QualificationCacheOwnerAggregate,
    /// Composite containing the cache setup and restore sequence.
    QualificationCachePrepareComposite,
    /// Composite immediately before one workflow-scope cache save.
    QualificationCacheBeforeSaveComposite,
    /// Composite immediately after one workflow-scope cache save.
    QualificationCacheAfterSaveComposite,
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
        if let Some(operation) = internal_operation_for_role(self) {
            return internal_operation(kind, operation);
        }
        match self {
            Self::Checkout => valid_checkout_payload(kind),
            Self::ToolSeed => valid_tool_seed_payload(kind),
            Self::MiseSetup => valid_mise_setup(kind),
            Self::TofuProviderUse => super::super::step_protocol::valid_provider_use(kind),
            Self::DownloadPlan | Self::AttestationDownload | Self::PreseedDownload => {
                action_has_prefix_for_kind(kind, "actions/download-artifact@")
            }
            Self::PublishPlan
            | Self::MatrixReportUpload
            | Self::PublishFinal
            | Self::QualificationCacheReceiptUpload
            | Self::PreseedUpload => action_has_prefix_for_kind(kind, "actions/upload-artifact@"),
            Self::ToolsCacheSave | Self::CargoSourcesSave => {
                action_has_prefix_for_kind(kind, "actions/cache/save@")
            }
            Self::ToolsCacheIdentity => valid_tools_cache_identity(kind),
            Self::QualificationCacheRestore => {
                action_has_prefix_for_kind(kind, "actions/cache/restore@")
            }
            Self::QualificationCacheSave => action_has_prefix_for_kind(kind, "actions/cache/save@"),
            Self::ToolsCacheRestore => matches!(
                kind,
                StepKind::Action { uses, .. } if uses == TOOLS_CACHE_RESTORE_USES
            ),
            Self::CacheTarLaunchers => valid_cache_tar_launcher(kind),
            Self::CargoSourcesRestore => action_has_prefix_for_kind(kind, "actions/cache/restore@"),
            Self::TofuProvidersRestore => super::super::step_protocol::valid_provider_restore(kind),
            Self::TofuProvidersSave => super::super::step_protocol::valid_provider_save(kind),
            Self::PrivateTokenMint => matches!(kind,
                StepKind::Action { uses, .. } if uses.starts_with(PRIVATE_TOKEN_MINT_ACTION_PREFIX)),
            Self::QualificationCachePrepareComposite
            | Self::QualificationCacheBeforeSaveComposite
            | Self::QualificationCacheAfterSaveComposite => {
                matches!(
                    kind,
                    StepKind::Action { uses, .. }
                        if uses.starts_with("./.github/actions/qualification-cache-")
                )
            }
            Self::MbxCache => valid_mbx_cache(kind),
            Self::PlaywrightInstall => super::super::step_protocol::valid_playwright_install(kind),
            Self::MbxVersionCheck | Self::MbxWorkspaceCleanup => {
                matches!(kind, StepKind::Shell { .. })
            }
            Self::PreparePinnedTools
            | Self::PrepareRustComponents
            | Self::CargoDeny
            | Self::CargoMachete
            | Self::Actionlint
            | Self::Zizmor
            | Self::Markdownlint
            | Self::StrictJson
            | Self::FrontmatterId
            | Self::LinkCheck
            | Self::NativeValidators
            | Self::AcquireVelnor
            | Self::PreseedStage
            | Self::CheckGenerated
            | Self::PlanFormat
            | Self::MsrvQualification
            | Self::CargoSourcesFetch
            | Self::CacheKeys
            | Self::PreseedBuild
            | Self::PreseedManifest
            | Self::PreseedVerifyBuild
            | Self::PreseedVerifyManifest
            | Self::MbxPreflight => matches!(kind, StepKind::Shell { .. }),
            _ => false,
        }
    }

    /// Output identity required for a role that publishes step outputs.
    pub(crate) const fn required_id(self) -> Option<StepId> {
        match self {
            Self::PlanProducer => Some(StepId::Plan),
            Self::BaselinePublisher => Some(StepId::PublishBaseline),
            Self::ToolsCacheIdentity => Some(StepId::ToolsCacheIdentity),
            Self::ToolsCacheRestore => Some(StepId::ToolsRestore),
            Self::NormalToolsCacheStage => Some(StepId::NormalToolsCacheStage),
            Self::NormalToolsCacheImport => Some(StepId::NormalToolsCacheImport),
            Self::NormalToolsCacheExport => Some(StepId::NormalToolsCacheExport),
            Self::CacheTarLaunchers => Some(StepId::CacheTarLaunchers),
            Self::PreseedStage => Some(StepId::PreseedStage),
            Self::TofuProvidersRestore => Some(StepId::TofuProviders),
            Self::MbxVersionCheck => Some(StepId::MbxReady),
            Self::CacheKeys => Some(StepId::CacheKeys),
            Self::PrivateTokenMint => Some(StepId::DepsToken),
            Self::QualificationCacheReceipt => Some(StepId::QualificationCacheReceipt),
            _ => None,
        }
    }

    /// Role required for a typed step output identity.
    pub(crate) const fn for_id(id: StepId) -> Self {
        match id {
            StepId::Plan => Self::PlanProducer,
            StepId::PublishBaseline => Self::BaselinePublisher,
            StepId::ToolsCacheIdentity => Self::ToolsCacheIdentity,
            StepId::NormalToolsCacheStage => Self::NormalToolsCacheStage,
            StepId::NormalToolsCacheImport => Self::NormalToolsCacheImport,
            StepId::NormalToolsCacheExport => Self::NormalToolsCacheExport,
            StepId::CacheTarLaunchers => Self::CacheTarLaunchers,
            StepId::PreseedStage => Self::PreseedStage,
            StepId::TofuProviders => Self::TofuProvidersRestore,
            StepId::MbxReady => Self::MbxVersionCheck,
            StepId::CacheKeys => Self::CacheKeys,
            StepId::SourcesRestore => Self::CargoSourcesRestore,
            StepId::SourcesFetch => Self::CargoSourcesFetch,
            StepId::ToolsRestore => Self::ToolsCacheRestore,
            StepId::DepsToken => Self::PrivateTokenMint,
            StepId::QualificationCache(identity) => match identity.stage() {
                super::super::step_id::QualificationCacheStepStage::RuntimeIdentity => {
                    Self::QualificationRuntimeIdentity
                }
                super::super::step_id::QualificationCacheStepStage::RuntimeIdentityAfterImport => {
                    Self::QualificationCacheRuntimeIdentityAfterImport
                }
                super::super::step_id::QualificationCacheStepStage::Roots => {
                    Self::QualificationCacheRoots
                }
                super::super::step_id::QualificationCacheStepStage::Bind => {
                    Self::QualificationCacheBind
                }
                super::super::step_id::QualificationCacheStepStage::Restore => {
                    Self::QualificationCacheRestore
                }
                super::super::step_id::QualificationCacheStepStage::Save => {
                    Self::QualificationCacheSave
                }
                super::super::step_id::QualificationCacheStepStage::RestoreCensus
                | super::super::step_id::QualificationCacheStepStage::SaveBeforeCensus
                | super::super::step_id::QualificationCacheStepStage::SaveAfterCensus => {
                    Self::QualificationCacheCensus
                }
                super::super::step_id::QualificationCacheStepStage::Import => {
                    Self::QualificationCachePayloadImport
                }
                super::super::step_id::QualificationCacheStepStage::Export => {
                    Self::QualificationCachePayloadExport
                }
                super::super::step_id::QualificationCacheStepStage::OwnerLayerEvidence => {
                    Self::QualificationCacheOwnerLayerEvidence
                }
                super::super::step_id::QualificationCacheStepStage::OwnerAggregate => {
                    Self::QualificationCacheOwnerAggregate
                }
            },
            StepId::QualificationCacheComposite(identity) => match identity.phase() {
                super::super::step_id::QualificationCacheCompositePhase::Prepare => {
                    Self::QualificationCachePrepareComposite
                }
                super::super::step_id::QualificationCacheCompositePhase::BeforeSave => {
                    Self::QualificationCacheBeforeSaveComposite
                }
                super::super::step_id::QualificationCacheCompositePhase::AfterSave => {
                    Self::QualificationCacheAfterSaveComposite
                }
            },
            StepId::QualificationRuntimeIdentity => Self::QualificationRuntimeIdentity,
            StepId::QualificationCacheReceipt => Self::QualificationCacheReceipt,
        }
    }
}

fn internal_operation_for_role(role: StepRole) -> Option<&'static str> {
    match role {
        StepRole::PlanProducer => Some("plan-v1"),
        StepRole::BaselinePublisher => Some("publish-baseline-v1"),
        StepRole::FetchReports => Some("fetch-reports-v1"),
        StepRole::NormalToolsCacheStage => Some(NORMAL_TOOLS_CACHE_STAGE_OPERATION),
        StepRole::NormalToolsCacheImport => Some(NORMAL_TOOLS_CACHE_IMPORT_OPERATION),
        StepRole::NormalToolsCacheExport => Some(NORMAL_TOOLS_CACHE_EXPORT_OPERATION),
        StepRole::PrivateFetch => Some(PRIVATE_FETCH_OPERATION),
        StepRole::PrivateRevoke => Some(PRIVATE_REVOKE_OPERATION),
        StepRole::BunRegistryInstall => Some(BUN_REGISTRY_INSTALL_OPERATION),
        StepRole::QualificationRuntimeIdentity
        | StepRole::QualificationCacheRuntimeIdentityAfterImport => {
            Some("observe-qualification-cache-runtime-v1")
        }
        StepRole::QualificationCacheRoots => Some("observe-qualification-cache-roots-v1"),
        StepRole::QualificationCacheBind => Some("bind-qualification-cache-v1"),
        StepRole::QualificationCacheImport | StepRole::QualificationCachePayloadImport => {
            Some("import-qualification-cache-payload-v1")
        }
        StepRole::QualificationCacheObservation | StepRole::QualificationCacheCensus => {
            Some("observe-qualification-cache-backend-v1")
        }
        StepRole::QualificationCacheExport | StepRole::QualificationCachePayloadExport => {
            Some("export-qualification-cache-payload-v1")
        }
        StepRole::QualificationCacheOwnerLayerEvidence => {
            Some("record-qualification-cache-owner-layer-v1")
        }
        StepRole::QualificationCacheOwnerAggregate => {
            Some("aggregate-qualification-cache-owner-v1")
        }
        StepRole::QualificationCacheReceipt => Some("write-qualification-cache-receipt-v1"),
        _ => None,
    }
}
