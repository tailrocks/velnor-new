use std::fmt;

use crate::metadata::MetadataError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseError {
    Metadata(MetadataError),
    FactsInvalid {
        manifest: String,
        detail: String,
    },
    UnknownPackage {
        name: String,
    },
    DuplicateSelection {
        name: String,
    },
    InvalidSelectionName {
        name: String,
    },
    NotWorkspaceMember {
        name: String,
    },
    AmbiguousPackageName {
        name: String,
    },
    PublishForbidden {
        package: String,
    },
    UnsupportedRegistry {
        package: String,
        registry: String,
    },
    InvalidRegistry {
        package: String,
        registry: String,
    },
    InvalidVersion {
        package: String,
        version: String,
    },
    InvalidPackageName {
        name: String,
    },
    ManifestEscape {
        package: String,
        manifest: String,
    },
    ManifestOutsideRoot {
        package: String,
        manifest: String,
    },
    UnresolvedLocalDep {
        package: String,
        dep: String,
        path: String,
    },
    InvalidRequirement {
        package: String,
        dep: String,
        req: String,
    },
    RequirementMismatch {
        package: String,
        dep: String,
        req: String,
        found: String,
    },
    GitOnlyDep {
        package: String,
        dep: String,
    },
    MissingRegistryVersion {
        package: String,
        dep: String,
        req: String,
    },
    UnpublishedLocalDep {
        dependent: String,
        dep: String,
        req: String,
    },
    UnsupportedSource {
        package: String,
        dep: String,
        source: String,
    },
    PublishCycle {
        members: Vec<String>,
    },
    MetadataMismatch {
        detail: String,
    },
    NothingSelected,
    InvalidTagPattern {
        pattern: String,
    },
    UnknownGroupMember {
        group: String,
        member: String,
    },
    OverlappingGroups {
        member: String,
    },
    EmptyGroup {
        group: String,
    },
    InvalidGroupName {
        group: String,
    },
    InvalidGroupVersion {
        group: String,
        member: String,
        version: String,
    },
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(err) => write!(f, "{err}"),
            Self::FactsInvalid { manifest, detail } => {
                write!(f, "release_facts_invalid:{manifest}: {detail}")
            }
            Self::UnknownPackage { name } => write!(f, "unknown_package:{name}"),
            Self::DuplicateSelection { name } => write!(f, "duplicate_selection:{name}"),
            Self::InvalidSelectionName { name } => write!(f, "invalid_selection:{name}"),
            Self::NotWorkspaceMember { name } => write!(f, "not_workspace_member:{name}"),
            Self::AmbiguousPackageName { name } => write!(f, "ambiguous_package:{name}"),
            Self::PublishForbidden { package } => write!(f, "publish_forbidden:{package}"),
            Self::UnsupportedRegistry { package, registry } => {
                write!(f, "unsupported_registry:{package}:{registry}")
            }
            Self::InvalidRegistry { package, registry } => {
                write!(f, "invalid_registry:{package}:{registry}")
            }
            Self::InvalidVersion { package, version } => {
                write!(f, "invalid_version:{package}:{version}")
            }
            Self::InvalidPackageName { name } => write!(f, "invalid_package_name:{name}"),
            Self::ManifestEscape { package, manifest } => {
                write!(f, "manifest_escape:{package}:{manifest}")
            }
            Self::ManifestOutsideRoot { package, manifest } => {
                write!(f, "manifest_outside_root:{package}:{manifest}")
            }
            Self::UnresolvedLocalDep { package, dep, path } => {
                write!(f, "unresolved_local_dep:{package}:{dep}: {path}")
            }
            Self::InvalidRequirement { package, dep, req } => {
                write!(f, "invalid_requirement:{package}:{dep}: {req}")
            }
            Self::RequirementMismatch {
                package,
                dep,
                req,
                found,
            } => {
                write!(f, "requirement_mismatch:{package}:{dep}: {req} vs {found}")
            }
            Self::GitOnlyDep { package, dep } => write!(f, "git_only_dep:{package}:{dep}"),
            Self::MissingRegistryVersion { package, dep, req } => {
                write!(f, "missing_registry_version:{package}:{dep}: {req}")
            }
            Self::UnpublishedLocalDep {
                dependent,
                dep,
                req,
            } => write!(
                f,
                "unpublished_local_dep:{dependent}: local '{dep}' ({req}) is neither \
                 selected nor published; add \"{dep}\" to release.packages"
            ),
            Self::UnsupportedSource {
                package,
                dep,
                source,
            } => {
                write!(f, "unsupported_source:{package}:{dep}: {source}")
            }
            Self::PublishCycle { members } => write!(f, "publish_cycle:{}", members.join(",")),
            Self::MetadataMismatch { detail } => write!(f, "metadata_mismatch: {detail}"),
            Self::NothingSelected => write!(f, "nothing_selected"),
            Self::InvalidTagPattern { pattern } => write!(f, "invalid_tag_pattern:{pattern}"),
            Self::UnknownGroupMember { group, member } => {
                write!(f, "unknown_group_member:{group}:{member}")
            }
            Self::OverlappingGroups { member } => write!(f, "overlapping_groups:{member}"),
            Self::EmptyGroup { group } => write!(f, "empty_group:{group}"),
            Self::InvalidGroupName { group } => write!(f, "invalid_group_name:{group}"),
            Self::InvalidGroupVersion {
                group,
                member,
                version,
            } => {
                write!(f, "invalid_group_version:{group}:{member}:{version}")
            }
        }
    }
}

impl std::error::Error for ReleaseError {}
