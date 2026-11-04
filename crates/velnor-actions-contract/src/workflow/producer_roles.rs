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

pub(super) fn seed_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::Checkout
            | StepRole::NonInputGuard
            | StepRole::MiseSetup
            | StepRole::PreparePinnedTools
            | StepRole::PrepareRustComponents
            | StepRole::VerifyRustManifest
            | StepRole::AcquireVelnor
            | StepRole::CargoSourcesRestore
            | StepRole::CargoSourcesFetch
            | StepRole::MbxPreflight
            | StepRole::MbxCache
            | StepRole::MbxVersionCheck
            | StepRole::MbxWorkspaceCleanup
            | StepRole::MbxBundleKey
            | StepRole::MbxBundleRestore
            | StepRole::MbxBundleImport
            | StepRole::MbxProducerMark
            | StepRole::MbxSeedBuild
            | StepRole::MbxBundleExport
            | StepRole::MbxBundleSave
            | StepRole::MbxBundleRecord
            | StepRole::PrivateTokenMint
            | StepRole::PrivateFetch
            | StepRole::PrivateRevoke
            | StepRole::ToolsCacheIdentity
            | StepRole::ToolsCacheRestore
            | StepRole::ToolsCacheSave
    )
}

pub(super) fn state_seal_build_role(role: StepRole) -> bool {
    matches!(role, StepRole::StateSealBuild | StepRole::StateSealUpload)
}

pub(super) fn state_seal_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::StateSealDownload
            | StepRole::StateSealSubject
            | StepRole::StateSealBind
            | StepRole::StateSealAttest
            | StepRole::StateSealSave
    )
}
