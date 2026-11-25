// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::steps::{HelperCommand, StepKind};

impl StepKind {
    #[must_use]
    pub const fn is_trusted_setup(&self) -> bool {
        match self {
            Self::Checkout { .. }
            | Self::InstallHelper(_)
            | Self::PublicSources { .. }
            | Self::RepositoryPrivacy
            | Self::PrivateCheck { .. }
            | Self::ReleaseAssets { .. }
            | Self::MbxRemote { .. }
            | Self::MbxPin
            | Self::MbxPrivate
            | Self::MbxStore
            | Self::MbxSeed
            | Self::CratesMirror(_) => true,
            Self::Helper(command) => match command {
                HelperCommand::Plan => true,
                HelperCommand::PlanFull
                | HelperCommand::CheckMetadata
                | HelperCommand::Restored(_)
                | HelperCommand::InputsAudit => false,
            },
            Self::InstallTools { .. }
            | Self::InstallMise
            | Self::Lint(_)
            | Self::CacheRestore(_)
            | Self::CacheLookup(_)
            | Self::CacheSave(_)
            | Self::RestoredTargets { .. }
            | Self::SourcesLocks(_)
            | Self::SourcesFetch { .. }
            | Self::SourcesFetchAll { .. }
            | Self::CacheDecide { .. }
            | Self::CacheResult(_)
            | Self::CacheVerify { .. }
            | Self::CacheTrim { .. }
            | Self::DeltaPrepare(_)
            | Self::DeltaApply(_)
            | Self::DeltaStage(_, _)
            | Self::CacheBudget(_)
            | Self::CachePurgePrivate
            | Self::RustComponents(_)
            | Self::RustTargets(_)
            | Self::UploadArchive { .. }
            | Self::UploadGroupArtifact(_)
            | Self::DownloadArchive { .. }
            | Self::Cargo(_)
            | Self::Prepare(_)
            | Self::InputsAuditRoot(_)
            | Self::InputsSave { .. }
            | Self::InputsRestore { .. }
            | Self::TestGroupsCheck(_)
            | Self::PrivateRecheck(_)
            | Self::PrivateToken { .. }
            | Self::PrivateFetch { .. }
            | Self::AdvisoryFetch { .. }
            | Self::MbxOutDirs
            | Self::ToolsLocal(..)
            | Self::TestSetup { .. }
            | Self::GeneratorCheck { .. }
            | Self::InstallGenerator(_)
            | Self::FailWhenANeedFailed
            | Self::FailOnDraftRun
            | Self::DraftNotice
            | Self::ClearRunLabel
            | Self::Verdict { .. }
            | Self::UploadCargoLog
            | Self::ReportCaches { .. }
            | Self::UploadMetrics
            | Self::DownloadMetrics
            | Self::CheckMetrics { .. }
            | Self::Ansible { .. }
            | Self::Budget { .. }
            | Self::MergeGroupPassed { .. }
            | Self::MergeQueueCheck => false,
        }
    }
}
