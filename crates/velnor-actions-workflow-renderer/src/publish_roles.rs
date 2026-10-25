use crate::release_permissions::PermissionLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublishRole {
    Authority,
    BranchAuthority,
    TagRelease,
    ArtifactBuild,
    ArtifactAttest,
    ArtifactPublish,
    ImagePublish,
    ImagePromote,
    NpmPublish,
}

impl PublishRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authority => "authority",
            Self::BranchAuthority => "branch-authority",
            Self::TagRelease => "tag-release",
            Self::ArtifactBuild => "artifact-build",
            Self::ArtifactAttest => "artifact-attest",
            Self::ArtifactPublish => "artifact-publish",
            Self::ImagePublish => "image-publish",
            Self::ImagePromote => "image-promote",
            Self::NpmPublish => "npm-publish",
        }
    }

    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Authority | Self::BranchAuthority => 0,
            Self::TagRelease | Self::ArtifactBuild | Self::ImagePublish | Self::ImagePromote => 1,
            Self::ArtifactAttest => 2,
            Self::ArtifactPublish => 3,
            Self::NpmPublish => 4,
        }
    }

    pub(crate) fn forbids_checkout(self) -> bool {
        !matches!(self, Self::ArtifactBuild | Self::TagRelease)
    }

    pub(crate) fn is_authority(self) -> bool {
        matches!(self, Self::Authority | Self::BranchAuthority)
    }

    pub(crate) fn gate(self) -> Option<Self> {
        match self {
            Self::Authority | Self::BranchAuthority => None,
            Self::TagRelease | Self::ImagePublish => Some(Self::BranchAuthority),
            Self::ArtifactBuild
            | Self::ArtifactAttest
            | Self::ArtifactPublish
            | Self::NpmPublish
            | Self::ImagePromote => Some(Self::Authority),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishPermissions {
    pub contents: PermissionLevel,
    pub checks: PermissionLevel,
    pub id_token: PermissionLevel,
    pub attestations: PermissionLevel,
    pub packages: PermissionLevel,
    pub actions: PermissionLevel,
    pub pull_requests: PermissionLevel,
}

impl PublishPermissions {
    #[must_use]
    pub fn expected(role: PublishRole) -> Self {
        use PermissionLevel::{None, Read, Write};
        let (contents, checks, id_token, attestations, packages, actions, pull_requests) =
            match role {
                PublishRole::Authority | PublishRole::BranchAuthority => {
                    (Read, Read, None, None, None, None, None)
                }
                PublishRole::ArtifactBuild => (Read, None, None, None, None, None, None),
                PublishRole::ArtifactAttest => (Read, None, Write, Write, None, None, None),
                PublishRole::TagRelease => (Write, None, None, None, None, None, Read),
                PublishRole::ArtifactPublish => (Write, None, None, None, None, None, None),
                PublishRole::ImagePublish => (Read, None, Write, Write, Write, Read, None),
                PublishRole::ImagePromote => (Read, None, None, Read, Write, None, None),
                PublishRole::NpmPublish => (Read, None, None, None, Write, None, None),
            };
        Self {
            contents,
            checks,
            id_token,
            attestations,
            packages,
            actions,
            pull_requests,
        }
    }
}
