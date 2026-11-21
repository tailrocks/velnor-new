// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Checkout,
    Mise,
    UploadArtifact,
    DownloadArtifact,
    Cache,
    CacheRestore,
    CacheSave,
    AttestBuildProvenance,
    GithubScript,
    MrBoxington,
    DockerLogin,
    DockerBuildPush,
    CreateGithubAppToken,
}

impl Action {
    pub const NAMES: &[&'static str] = &[
        "checkout",
        "mise",
        "upload-artifact",
        "download-artifact",
        "cache",
        "cache-restore",
        "cache-save",
        "attest",
        "github-script",
        "mr-boxington",
        "docker-login",
        "docker-build-push",
        "create-github-app-token",
    ];

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "checkout" => Self::Checkout,
            "mise" => Self::Mise,
            "upload-artifact" => Self::UploadArtifact,
            "download-artifact" => Self::DownloadArtifact,
            "cache" => Self::Cache,
            "cache-restore" => Self::CacheRestore,
            "cache-save" => Self::CacheSave,
            "attest" => Self::AttestBuildProvenance,
            "github-script" => Self::GithubScript,
            "mr-boxington" => Self::MrBoxington,
            "docker-login" => Self::DockerLogin,
            "docker-build-push" => Self::DockerBuildPush,
            "create-github-app-token" => Self::CreateGithubAppToken,
            _ => return None,
        })
    }

    #[must_use]
    pub fn locked_off(self) -> &'static [&'static str] {
        match self {
            Self::Checkout => &["persist-credentials"],
            Self::Mise => &["cache", "cache_save"],
            Self::AttestBuildProvenance => &["push-to-registry"],
            Self::UploadArtifact
            | Self::DownloadArtifact
            | Self::Cache
            | Self::CacheRestore
            | Self::CacheSave
            | Self::GithubScript
            | Self::MrBoxington
            | Self::DockerLogin
            | Self::DockerBuildPush
            | Self::CreateGithubAppToken => &[],
        }
    }

    /// The job permissions the action cannot run without.
    #[must_use]
    pub fn needs_permissions(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Checkout => &[("contents", "read")],
            Self::AttestBuildProvenance => &[
                ("contents", "read"),
                ("id-token", "write"),
                ("attestations", "write"),
            ],
            _ => &[],
        }
    }
}
