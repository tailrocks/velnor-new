use super::super::super::step_id::{QualificationCacheCompositePhase, QualificationCacheStepStage};
use super::super::StepId;
use super::StepRole;

impl StepRole {
    /// Output identity required for a role that publishes step outputs.
    pub(crate) const fn required_id(self) -> Option<StepId> {
        match self {
            Self::PlanProducer => Some(StepId::Plan),
            Self::ToolsCacheIdentity => Some(StepId::ToolsCacheIdentity),
            Self::ToolsCacheRestore => Some(StepId::ToolsRestore),
            Self::NormalToolsCacheStage => Some(StepId::NormalToolsCacheStage),
            Self::NormalToolsCacheImport => Some(StepId::NormalToolsCacheImport),
            Self::NormalToolsCacheExport => Some(StepId::NormalToolsCacheExport),
            Self::CacheTarLaunchers => Some(StepId::CacheTarLaunchers),
            Self::PreseedStage => Some(StepId::PreseedStage),
            Self::TofuProvidersRestore => Some(StepId::TofuProviders),
            Self::MbxVersionCheck => Some(StepId::MbxReady),
            Self::MbxBundleKey => Some(StepId::MbxBundleKey),
            Self::MbxBundleRestore => Some(StepId::MbxBundleRestore),
            Self::MbxBundleExport => Some(StepId::MbxBundleExport),
            Self::CacheKeys => Some(StepId::CacheKeys),
            Self::PrivateTokenMint => Some(StepId::DepsToken),
            Self::QualificationCacheReceipt => Some(StepId::QualificationCacheReceipt),
            Self::FamilyKeys => Some(StepId::FamilyKeys),
            _ => None,
        }
    }

    /// Role required for a typed step output identity.
    pub(crate) const fn for_id(id: StepId) -> Self {
        match id {
            StepId::Plan => Self::PlanProducer,
            StepId::ToolsCacheIdentity => Self::ToolsCacheIdentity,
            StepId::NormalToolsCacheStage => Self::NormalToolsCacheStage,
            StepId::NormalToolsCacheImport => Self::NormalToolsCacheImport,
            StepId::NormalToolsCacheExport => Self::NormalToolsCacheExport,
            StepId::CacheTarLaunchers => Self::CacheTarLaunchers,
            StepId::PreseedStage => Self::PreseedStage,
            StepId::TofuProviders => Self::TofuProvidersRestore,
            StepId::MbxReady => Self::MbxVersionCheck,
            StepId::MbxBundleKey => Self::MbxBundleKey,
            StepId::MbxBundleRestore => Self::MbxBundleRestore,
            StepId::MbxBundleExport => Self::MbxBundleExport,
            StepId::CacheKeys => Self::CacheKeys,
            StepId::SourcesRestore => Self::CargoSourcesRestore,
            StepId::SourcesFetch => Self::CargoSourcesFetch,
            StepId::ToolsRestore => Self::ToolsCacheRestore,
            StepId::DepsToken => Self::PrivateTokenMint,
            StepId::QualificationCache(identity) => match identity.stage() {
                QualificationCacheStepStage::RuntimeIdentity => Self::QualificationRuntimeIdentity,
                QualificationCacheStepStage::RuntimeIdentityAfterImport => {
                    Self::QualificationCacheRuntimeIdentityAfterImport
                }
                QualificationCacheStepStage::Roots => Self::QualificationCacheRoots,
                QualificationCacheStepStage::Bind => Self::QualificationCacheBind,
                QualificationCacheStepStage::Restore => Self::QualificationCacheRestore,
                QualificationCacheStepStage::Save => Self::QualificationCacheSave,
                QualificationCacheStepStage::RestoreCensus
                | QualificationCacheStepStage::SaveBeforeCensus
                | QualificationCacheStepStage::SaveAfterCensus => Self::QualificationCacheCensus,
                QualificationCacheStepStage::Import => Self::QualificationCachePayloadImport,
                QualificationCacheStepStage::Export => Self::QualificationCachePayloadExport,
                QualificationCacheStepStage::OwnerLayerEvidence => {
                    Self::QualificationCacheOwnerLayerEvidence
                }
                QualificationCacheStepStage::OwnerAggregate => {
                    Self::QualificationCacheOwnerAggregate
                }
            },
            StepId::QualificationCacheComposite(identity) => match identity.phase() {
                QualificationCacheCompositePhase::Prepare => {
                    Self::QualificationCachePrepareComposite
                }
                QualificationCacheCompositePhase::BeforeSave => {
                    Self::QualificationCacheBeforeSaveComposite
                }
                QualificationCacheCompositePhase::AfterSave => {
                    Self::QualificationCacheAfterSaveComposite
                }
            },
            StepId::QualificationRuntimeIdentity => Self::QualificationRuntimeIdentity,
            StepId::QualificationCacheReceipt => Self::QualificationCacheReceipt,
            StepId::FamilyKeys => Self::FamilyKeys,
            StepId::FamilySizeBunInstall
            | StepId::FamilySizePlaywrightBrowsers
            | StepId::FamilySizeDbtPackages
            | StepId::FamilySizePythonVenv
            | StepId::FamilySizeAnsibleCollections => Self::FamilySizeCheck,
        }
    }
}
