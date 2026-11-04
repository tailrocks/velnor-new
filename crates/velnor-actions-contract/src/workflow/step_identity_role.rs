// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{step_mbx_lifecycle, step_protocol};
use super::{
    BUN_REGISTRY_INSTALL_OPERATION, ContractError, GITHUB_EVIDENCE_FETCH_OPERATION,
    GITHUB_EVIDENCE_IMPORT_OPERATION, PRIVATE_FETCH_OPERATION, PRIVATE_REVOKE_OPERATION,
    PRIVATE_TOKEN_MINT_ACTION_PREFIX, StepId, StepKind, StepRole, ToolsRestoreForm,
    action_has_prefix_for_kind, internal_operation, valid_bun_assets_handoff,
    valid_checkout_payload, valid_jactionlint_verifier_payload, valid_mise_setup,
    valid_rust_verifier_payload, valid_tool_seed_payload, valid_tools_cache_identity,
    valid_tools_cache_scrub,
};

impl StepRole {
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

    fn matches_payload(self, kind: &StepKind) -> bool {
        match self {
            Self::Checkout => valid_checkout_payload(kind),
            Self::ToolSeed => valid_tool_seed_payload(kind),
            Self::MiseSetup => valid_mise_setup(kind),
            Self::TofuProviderUse => step_protocol::valid_provider_use(kind),
            Self::PlanProducer => internal_operation(kind, "plan-v1"),
            Self::FetchReports => internal_operation(kind, "fetch-reports-v1"),
            Self::DownloadPlan
            | Self::AttestationDownload
            | Self::PreseedDownload
            | Self::StateSealDownload
            | Self::LedgerCliDownload => {
                action_has_prefix_for_kind(kind, "actions/download-artifact@")
            }
            Self::PublishPlan
            | Self::MatrixReportUpload
            | Self::PublishFinal
            | Self::PreseedUpload
            | Self::StateSealUpload
            | Self::LedgerCliUpload => action_has_prefix_for_kind(kind, "actions/upload-artifact@"),
            Self::StateSealAttest => action_has_prefix_for_kind(kind, "actions/attest@"),
            Self::ToolsCacheSave
            | Self::CargoSourcesSave
            | Self::StateSealSave
            | Self::FamilySave => action_has_prefix_for_kind(kind, "actions/cache/save@"),
            Self::ToolsCacheIdentity => valid_tools_cache_identity(kind),
            Self::ToolsCacheRestore => ToolsRestoreForm::of(kind).is_some(),
            Self::ToolsCacheScrub => valid_tools_cache_scrub(kind),
            Self::CargoSourcesRestore | Self::MbxBundleRestore | Self::FamilyRestore => {
                action_has_prefix_for_kind(kind, "actions/cache/restore@")
            }
            Self::TofuProvidersRestore => step_protocol::valid_provider_restore(kind),
            Self::TofuProvidersSave => step_protocol::valid_provider_save(kind),
            Self::TofuProvidersPrepare | Self::NonInputGuard => {
                matches!(kind, StepKind::Shell { .. })
            }
            Self::PrivateTokenMint => matches!(kind,
                StepKind::Action { uses, .. } if uses.starts_with(PRIVATE_TOKEN_MINT_ACTION_PREFIX)),
            Self::PrivateFetch => internal_operation(kind, PRIVATE_FETCH_OPERATION),
            Self::GitHubEvidenceFetch => internal_operation(kind, GITHUB_EVIDENCE_FETCH_OPERATION),
            Self::GitHubEvidenceImport => {
                internal_operation(kind, GITHUB_EVIDENCE_IMPORT_OPERATION)
            }
            Self::PrivateRevoke => internal_operation(kind, PRIVATE_REVOKE_OPERATION),
            Self::ReleaseRustUpload | Self::ReleaseReceiptUpload | Self::ReleasePrArtifact => {
                matches!(kind, StepKind::Action { uses, .. } if uses.starts_with("actions/upload-artifact@"))
            }
            Self::ReleasePrCandidate => matches!(kind, StepKind::Shell { .. }),
            Self::BunRegistryInstall => internal_operation(kind, BUN_REGISTRY_INSTALL_OPERATION),
            Self::BunAssetsHandoff => valid_bun_assets_handoff(kind),
            Self::MbxCache => step_mbx_lifecycle::valid_mbx_cache(kind),
            Self::PlaywrightInstall => step_protocol::valid_playwright_install(kind),
            Self::VerifyRustManifest => valid_rust_verifier_payload(kind),
            Self::VerifyJactionlint => valid_jactionlint_verifier_payload(kind),
            shell_only => shell_only.is_shell_only() && matches!(kind, StepKind::Shell { .. }),
        }
    }

    pub(crate) const fn required_id(self) -> Option<StepId> {
        match self {
            Self::ReleaseAdmission => Some(StepId::ReleaseAdmission),
            Self::ReleaseVerification => Some(StepId::ReleaseVerification),
            Self::ReleaseRustUpload => Some(StepId::ReleaseRustUpload),
            Self::ReleaseReceiptUpload => Some(StepId::ReleaseReceiptUpload),
            Self::ReleasePrCandidate => Some(StepId::ReleasePrCandidate),
            Self::ReleasePrArtifact => Some(StepId::ReleasePrArtifact),
            Self::PlanProducer => Some(StepId::Plan),
            Self::BuildTaskRun => Some(StepId::BuildTaskRun),
            Self::ToolsCacheIdentity => Some(StepId::ToolsCacheIdentity),
            Self::TofuProvidersRestore => Some(StepId::TofuProviders),
            Self::MbxVersionCheck => Some(StepId::MbxReady),
            Self::CacheKeys => Some(StepId::CacheKeys),
            Self::ToolsCacheRestore => Some(StepId::ToolsRestore),
            Self::PrivateTokenMint => Some(StepId::DepsToken),
            Self::MbxBundleKey => Some(StepId::MbxBundleKey),
            Self::MbxBundleImport => Some(StepId::MbxBundleImport),
            Self::MbxBundleExport => Some(StepId::MbxBundleExport),
            Self::FamilyKeys => Some(StepId::FamilyKeys),
            Self::LedgerCliPublish => Some(StepId::LedgerCliPublish),
            Self::LedgerCliVerify => Some(StepId::LedgerCliVerify),
            Self::StateSelect => Some(StepId::StateSelect),
            Self::StateSealSubject => Some(StepId::StateSealSubject),
            Self::StateSealBind => Some(StepId::StateSealBind),
            Self::StateSealTip => Some(StepId::StateSealTip),
            Self::StateSealUpload => Some(StepId::StateSealUpload),
            Self::StateSealAdmit => Some(StepId::StateSealAdmit),
            Self::StateSealSave => Some(StepId::StateSealSave),
            Self::StateProduced => Some(StepId::StateProduced),
            _ => None,
        }
    }

    pub(crate) const fn for_id(id: StepId) -> Self {
        match id {
            StepId::ReleaseAdmission => Self::ReleaseAdmission,
            StepId::ReleaseVerification => Self::ReleaseVerification,
            StepId::ReleaseRustUpload => Self::ReleaseRustUpload,
            StepId::ReleaseReceiptUpload => Self::ReleaseReceiptUpload,
            StepId::ReleasePrCandidate => Self::ReleasePrCandidate,
            StepId::ReleasePrArtifact => Self::ReleasePrArtifact,
            StepId::Plan => Self::PlanProducer,
            StepId::BuildTaskRun => Self::BuildTaskRun,
            StepId::ToolsCacheIdentity => Self::ToolsCacheIdentity,
            StepId::TofuProviders => Self::TofuProvidersRestore,
            StepId::MbxReady => Self::MbxVersionCheck,
            StepId::CacheKeys => Self::CacheKeys,
            StepId::SourcesRestore => Self::CargoSourcesRestore,
            StepId::ToolsRestore => Self::ToolsCacheRestore,
            StepId::DepsToken => Self::PrivateTokenMint,
            StepId::MbxBundleKey => Self::MbxBundleKey,
            StepId::MbxBundleRestore => Self::MbxBundleRestore,
            StepId::MbxBundleImport => Self::MbxBundleImport,
            StepId::MbxBundleExport => Self::MbxBundleExport,
            StepId::FamilyKeys => Self::FamilyKeys,
            StepId::LedgerCliPublish => Self::LedgerCliPublish,
            StepId::LedgerCliVerify => Self::LedgerCliVerify,
            StepId::StateSelect => Self::StateSelect,
            StepId::StateSealSubject => Self::StateSealSubject,
            StepId::StateSealBind => Self::StateSealBind,
            StepId::StateSealTip => Self::StateSealTip,
            StepId::StateSealUpload => Self::StateSealUpload,
            StepId::StateSealAdmit => Self::StateSealAdmit,
            StepId::StateSealSave => Self::StateSealSave,
            StepId::StateProduced => Self::StateProduced,
            StepId::FamilySizeBunInstall
            | StepId::FamilySizePlaywrightBrowsers
            | StepId::FamilySizeDbtPackages
            | StepId::FamilySizePythonVenv
            | StepId::FamilySizeAnsibleCollections => Self::FamilySizeCheck,
        }
    }
}
