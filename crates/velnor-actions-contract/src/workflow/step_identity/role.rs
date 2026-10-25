use super::super::step::StepKind;
use super::payload::{
    action_has_prefix_for_kind, internal_operation, valid_cache_tar_launcher,
    valid_jactionlint_verifier_payload, valid_rust_verifier_payload, valid_tools_cache_identity,
};
use super::{
    BUN_ASSETS_EXPORT_OPERATION, BUN_ASSETS_IMPORT_OPERATION, BUN_ASSETS_PREPARE_OPERATION,
    BUN_REGISTRY_INSTALL_OPERATION, GITHUB_EVIDENCE_FETCH_OPERATION,
    GITHUB_EVIDENCE_IMPORT_OPERATION, NORMAL_TOOLS_CACHE_EXPORT_OPERATION,
    NORMAL_TOOLS_CACHE_IMPORT_OPERATION, NORMAL_TOOLS_CACHE_STAGE_OPERATION,
    PRIVATE_FETCH_OPERATION, PRIVATE_REVOKE_OPERATION, PRIVATE_TOKEN_MINT_ACTION_PREFIX,
    TOOLS_CACHE_RESTORE_USES,
};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

mod identity;

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
    /// Verify the pinned Rust channel manifest and installed component hashes.
    VerifyRustManifest,
    /// Verify the installed jactionlint executable against Velnor's digest pin.
    VerifyJactionlint,
    /// Pinned Mise action that owns its built-in tools cache.
    MiseSetup,
    /// Authenticated cargo-deny analyzer bootstrap.
    CargoDeny,
    /// Authenticated cargo-machete analyzer bootstrap.
    CargoMachete,
    /// Workflow lint analyzer (jactionlint).
    Jactionlint,
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
    /// Calculate the cache key for an MBX bundle.
    MbxBundleKey,
    /// Restore an MBX bundle from the cache.
    MbxBundleRestore,
    /// Import a restored MBX bundle.
    MbxBundleImport,
    /// Record producer success before exporting an MBX bundle.
    MbxProducerMark,
    /// Compile one dependency seed lane through `mbx` for the seed job's producer.
    MbxSeedBuild,
    /// Export the MBX bundle.
    MbxBundleExport,
    /// Save the MBX bundle to the trusted cache.
    MbxBundleSave,
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
    /// Credential-free private asset archive handoff.
    BunAssetsHandoff,
    /// Hash-verified Playwright browser install; holds no credential.
    PlaywrightInstall,
    /// Trusted fixed-route Actions evidence acquisition; token held only here.
    GitHubEvidenceFetch,
    /// Credential-free verification and import of the same-run snapshot.
    GitHubEvidenceImport,
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
    /// Runtime trust and keys of generated stack-family caches.
    FamilyKeys,
    /// Read-only restore of one stack-family cache.
    FamilyRestore,
    /// On-disk size admission before saving one stack-family cache.
    FamilySizeCheck,
    /// Trusted-only save of one stack-family cache.
    FamilySave,
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
        if is_shell_role(self) {
            return matches!(kind, StepKind::Shell { .. });
        }
        match self {
            Self::VerifyRustManifest => valid_rust_verifier_payload(kind),
            Self::VerifyJactionlint => valid_jactionlint_verifier_payload(kind),
            Self::Checkout => super::super::step_payloads::valid_checkout_payload(kind),
            Self::ToolSeed => super::valid_tool_seed_payload(kind),
            Self::MiseSetup => super::valid_mise_setup(kind),
            Self::TofuProviderUse => super::super::step_protocol::valid_provider_use(kind),
            Self::DownloadPlan | Self::AttestationDownload | Self::PreseedDownload => {
                action_has_prefix_for_kind(kind, "actions/download-artifact@")
            }
            Self::PublishPlan
            | Self::MatrixReportUpload
            | Self::PublishFinal
            | Self::QualificationCacheReceiptUpload
            | Self::PreseedUpload => action_has_prefix_for_kind(kind, "actions/upload-artifact@"),
            Self::ToolsCacheSave
            | Self::CargoSourcesSave
            | Self::QualificationCacheSave
            | Self::MbxBundleSave
            | Self::FamilySave => action_has_prefix_for_kind(kind, "actions/cache/save@"),
            Self::ToolsCacheIdentity => valid_tools_cache_identity(kind),
            Self::QualificationCacheRestore
            | Self::CargoSourcesRestore
            | Self::MbxBundleRestore
            | Self::FamilyRestore => action_has_prefix_for_kind(kind, "actions/cache/restore@"),
            Self::ToolsCacheRestore => matches!(
                kind,
                StepKind::Action { uses, .. } if uses == TOOLS_CACHE_RESTORE_USES
            ),
            Self::CacheTarLaunchers => valid_cache_tar_launcher(kind),
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
            Self::BunAssetsHandoff => [
                BUN_ASSETS_EXPORT_OPERATION,
                BUN_ASSETS_PREPARE_OPERATION,
                BUN_ASSETS_IMPORT_OPERATION,
            ]
            .into_iter()
            .any(|operation| internal_operation(kind, operation)),
            Self::MbxCache => super::super::step_mbx_lifecycle::valid_mbx_cache(kind),
            Self::MbxBundleKey
            | Self::MbxBundleImport
            | Self::MbxProducerMark
            | Self::MbxSeedBuild
            | Self::MbxBundleExport
            | Self::FamilyKeys
            | Self::FamilySizeCheck => matches!(kind, StepKind::Shell { .. }),
            Self::PlaywrightInstall => super::super::step_protocol::valid_playwright_install(kind),
            Self::MbxVersionCheck | Self::MbxWorkspaceCleanup => {
                matches!(kind, StepKind::Shell { .. })
            }
            _ => false,
        }
    }
}

fn is_shell_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::PreparePinnedTools
            | StepRole::PrepareRustComponents
            | StepRole::CargoDeny
            | StepRole::CargoMachete
            | StepRole::Jactionlint
            | StepRole::Markdownlint
            | StepRole::StrictJson
            | StepRole::FrontmatterId
            | StepRole::LinkCheck
            | StepRole::NativeValidators
            | StepRole::AcquireVelnor
            | StepRole::PreseedStage
            | StepRole::CheckGenerated
            | StepRole::PlanFormat
            | StepRole::MsrvQualification
            | StepRole::CargoSourcesFetch
            | StepRole::CacheKeys
            | StepRole::PreseedBuild
            | StepRole::PreseedManifest
            | StepRole::PreseedVerifyBuild
            | StepRole::PreseedVerifyManifest
            | StepRole::MbxPreflight
    )
}

fn internal_operation_for_role(role: StepRole) -> Option<&'static str> {
    match role {
        StepRole::PlanProducer => Some("plan-v1"),
        StepRole::FetchReports => Some("fetch-reports-v1"),
        StepRole::NormalToolsCacheStage => Some(NORMAL_TOOLS_CACHE_STAGE_OPERATION),
        StepRole::NormalToolsCacheImport => Some(NORMAL_TOOLS_CACHE_IMPORT_OPERATION),
        StepRole::NormalToolsCacheExport => Some(NORMAL_TOOLS_CACHE_EXPORT_OPERATION),
        StepRole::PrivateFetch => Some(PRIVATE_FETCH_OPERATION),
        StepRole::GitHubEvidenceFetch => Some(GITHUB_EVIDENCE_FETCH_OPERATION),
        StepRole::GitHubEvidenceImport => Some(GITHUB_EVIDENCE_IMPORT_OPERATION),
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
