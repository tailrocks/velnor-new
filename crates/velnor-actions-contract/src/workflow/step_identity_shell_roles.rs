// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{StepKind, StepRole};

impl StepRole {
    pub(super) fn valid_runtime_tools(kind: &StepKind) -> bool {
        let StepKind::Shell { run, .. } = kind else {
            return false;
        };
        matches!(run.as_slice(), [mise, no_config, no_env, no_hooks, install, tool]
            if mise == "mise" && no_config == "--no-config" && no_env == "--no-env"
                && no_hooks == "--no-hooks" && install == "install"
                && matches!(tool.as_str(), "gh@2.103.2" | "gh@2.103.3"))
    }
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
                | Self::RunnerFamilySourcesPrefetch
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
                | Self::StateSealBind
                | Self::StateSealTip
                | Self::NonInputGuard
                | Self::MbxStats
        )
    }
}
