// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::StepRole;

pub(super) fn sources_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::Checkout
            | StepRole::AcquireVelnor
            | StepRole::MiseSetup
            | StepRole::ToolsCacheIdentity
            | StepRole::ToolsCacheRestore
            | StepRole::PreparePinnedTools
            | StepRole::PrepareRustComponents
            | StepRole::VerifyRustManifest
            | StepRole::VerifyJactionlint
            | StepRole::CargoSourcesRestore
            | StepRole::CargoSourcesFetch
            | StepRole::CargoSourcesSave
    )
}

pub(super) fn tofu_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::Checkout
            | StepRole::MiseSetup
            | StepRole::PreparePinnedTools
            | StepRole::TofuProvidersRestore
            | StepRole::TofuProvidersPrepare
            | StepRole::TofuProviderUse
            | StepRole::TofuProvidersSave
    )
}

pub(super) fn tools_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::Checkout
            | StepRole::MiseSetup
            | StepRole::ToolsCacheIdentity
            | StepRole::ToolsCacheRestore
            | StepRole::ToolsCacheScrub
            | StepRole::PreparePinnedTools
            | StepRole::PrepareRustComponents
            | StepRole::VerifyRustManifest
            | StepRole::VerifyJactionlint
            | StepRole::ToolsCacheSave
    )
}

pub(super) fn state_producer_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::Checkout
            | StepRole::NonInputGuard
            | StepRole::MiseSetup
            | StepRole::PreparePinnedTools
            | StepRole::PrepareRustComponents
            | StepRole::VerifyRustManifest
            | StepRole::AcquireVelnor
            | StepRole::DownloadPlan
            | StepRole::CargoSourcesRestore
            | StepRole::CargoSourcesFetch
            | StepRole::MbxPreflight
            | StepRole::MbxCache
            | StepRole::MbxVersionCheck
            | StepRole::MbxWorkspaceCleanup
            | StepRole::MbxBundleKey
            | StepRole::MbxBundleRestore
            | StepRole::MbxBundleImport
            | StepRole::StateSkeleton
            | StepRole::StateSkeletonRestore
            | StepRole::MbxProducerMark
            | StepRole::MbxBundleExport
            | StepRole::StateSealUpload
            | StepRole::StateProduced
            | StepRole::PrivateTokenMint
            | StepRole::PrivateFetch
            | StepRole::PrivateRevoke
            | StepRole::ToolsCacheIdentity
            | StepRole::ToolsCacheRestore
            | StepRole::ToolsCacheSave
    )
}

pub(super) fn state_seal_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::StateSealDownload
            | StepRole::StateSealSubject
            | StepRole::StateSealAdmit
            | StepRole::StateSealBind
            | StepRole::StateSealAttest
            | StepRole::StateSealSave
            | StepRole::StateSealRetire
    )
}
