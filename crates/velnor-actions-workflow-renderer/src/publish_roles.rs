//! Publication boundary roles and their exact least-privilege permission matrix.

use crate::release_permissions::PermissionLevel;

/// Publication boundary role carried by one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PublishRole {
    /// Verifies tag release authority; reads GitHub metadata only, no checkout.
    Authority,
    /// Verifies default-branch push authority; waits for `Required`, no checkout.
    BranchAuthority,
    /// Creates tags and GitHub releases from the verified default-branch commit.
    TagRelease,
    /// Builds an artifact from the checked-out tag commit; read-only.
    ArtifactBuild,
    /// Attests the built artifact; holds OIDC and attestation authority, no checkout.
    ArtifactAttest,
    /// Uploads the attested artifact as a release asset; no checkout.
    ArtifactPublish,
    /// Pushes built image layouts to the registry and attests the digests; no checkout.
    ImagePublish,
    /// Re-tags an already attested image digest after verifying it; no checkout.
    ImagePromote,
    /// Pushes an attested npm tarball to GitHub Packages; no checkout.
    NpmPublish,
}

impl PublishRole {
    /// Machine-readable role name for errors.
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

    /// Forward-only pipeline rank (needs must point to lower ranks).
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Authority | Self::BranchAuthority => 0,
            Self::TagRelease | Self::ArtifactBuild | Self::ImagePublish | Self::ImagePromote => 1,
            Self::ArtifactAttest => 2,
            Self::ArtifactPublish | Self::NpmPublish => 3,
        }
    }

    /// True for roles that execute no repository code and never check it out.
    ///
    /// `ArtifactBuild` only reads files; `TagRelease` checks out so the pinned
    /// release tool can read manifests and history, and runs no repository code.
    pub(crate) fn forbids_checkout(self) -> bool {
        !matches!(self, Self::ArtifactBuild | Self::TagRelease)
    }

    /// True for the roles that gate the rest of the workflow.
    pub(crate) fn is_authority(self) -> bool {
        matches!(self, Self::Authority | Self::BranchAuthority)
    }

    /// The authority role a downstream role must transitively need.
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

/// Per-job least-privilege permissions (typed levels only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishPermissions {
    /// Repository contents access.
    pub contents: PermissionLevel,
    /// Check-run read access (the `Required` verdict).
    pub checks: PermissionLevel,
    /// OIDC token access (attestation signing).
    pub id_token: PermissionLevel,
    /// Artifact attestation access.
    pub attestations: PermissionLevel,
    /// GitHub Packages (registry) access.
    pub packages: PermissionLevel,
    /// Actions access (reading the artifacts of the push's CI run).
    pub actions: PermissionLevel,
    /// Pull-request read access (`release-plz release` lists a commit's pull requests).
    pub pull_requests: PermissionLevel,
}

impl PublishPermissions {
    /// Exact least-privilege matrix for one role.
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
