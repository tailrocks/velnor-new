// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::StepRole;

impl StepRole {
    pub(super) const fn is_shell_only(self) -> bool {
        matches!(
            self,
            Self::PreparePinnedTools
                | Self::MbxVersionCheck
                | Self::MbxWorkspaceCleanup
                | Self::MbxBundleKey
                | Self::FamilyKeys
                | Self::FamilySizeCheck
                | Self::MbxBundleImport
                | Self::MbxProducerMark
                | Self::MbxBundleExport
                | Self::MbxBundleRecord
                | Self::PrepareRustComponents
                | Self::CargoDeny
                | Self::CargoMachete
                | Self::Jactionlint
                | Self::Markdownlint
                | Self::StrictJson
                | Self::FrontmatterId
                | Self::LinkCheck
                | Self::NativeValidators
                | Self::AcquireVelnor
                | Self::BuildTaskRun
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
                | Self::MbxPreflight
                | Self::LedgerCliBuild
                | Self::LedgerCliPublish
                | Self::LedgerCliVerify
                | Self::StateSelect
                | Self::StateSealSubject
                | Self::StateSealAdmit
                | Self::StateSealRetire
                | Self::StateProduced
                | Self::StateSkeleton
                | Self::StateSkeletonRestore
        )
    }
}
