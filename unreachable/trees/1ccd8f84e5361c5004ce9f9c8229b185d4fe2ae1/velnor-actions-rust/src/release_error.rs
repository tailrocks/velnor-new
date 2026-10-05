//! Release selection, graph, and emission failures.
//!
//! Every variant fails closed: unknown, ambiguous, or unverifiable input is
//! an error, never a silent skip or an expanded upload set.

use std::fmt;

use crate::metadata::MetadataError;

/// Release-set derivation, publication-graph, or emission failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseError {
    /// Authoritative metadata failed to parse.
    Metadata(MetadataError),
    /// Supplementary release facts failed to parse.
    FactsInvalid {
        /// Candidate manifest being parsed.
        manifest: String,
        /// Diagnostic detail.
        detail: String,
    },
    /// Selected name matches no workspace package.
    UnknownPackage {
        /// Requested name.
        name: String,
    },
    /// Selection lists the same name twice.
    DuplicateSelection {
        /// Repeated name.
        name: String,
    },
    /// Selection entry is not a valid package name.
    InvalidSelectionName {
        /// Offending entry.
        name: String,
    },
    /// Selected name is not a workspace member.
    NotWorkspaceMember {
        /// Requested name.
        name: String,
    },
    /// Two workspace members share one name.
    AmbiguousPackageName {
        /// Repeated name.
        name: String,
    },
    /// Selected package declares `publish = false`.
    PublishForbidden {
        /// Package name.
        package: String,
    },
    /// Package targets a registry the pipeline cannot publish to.
    UnsupportedRegistry {
        /// Package name.
        package: String,
        /// Restricted registry.
        registry: String,
    },
    /// A declared registry name is malformed.
    InvalidRegistry {
        /// Package name.
        package: String,
        /// Offending registry.
        registry: String,
    },
    /// A member version is not strict semver.
    InvalidVersion {
        /// Package name.
        package: String,
        /// Offending version.
        version: String,
    },
    /// A member name violates package-name rules.
    InvalidPackageName {
        /// Offending name.
        name: String,
    },
    /// A member manifest path escapes or is malformed.
    ManifestEscape {
        /// Package name.
        package: String,
        /// Offending path.
        manifest: String,
    },
    /// A member manifest lives outside the repository root.
    ManifestOutsideRoot {
        /// Package name.
        package: String,
        /// Offending path.
        manifest: String,
    },
    /// A path dependency matches no reported package.
    UnresolvedLocalDep {
        /// Dependent package name.
        package: String,
        /// Dependency name.
        dep: String,
        /// Declared path value.
        path: String,
    },
    /// A dependency requirement is not parseable.
    InvalidRequirement {
        /// Dependent package name.
        package: String,
        /// Dependency name.
        dep: String,
        /// Offending requirement.
        req: String,
    },
    /// A local target version violates the declared requirement.
    RequirementMismatch {
        /// Dependent package name.
        package: String,
        /// Dependency name.
        dep: String,
        /// Declared requirement.
        req: String,
        /// Target version found.
        found: String,
    },
    /// A selected package needs a Git-only dependency.
    GitOnlyDep {
        /// Dependent package name.
        package: String,
        /// Dependency name.
        dep: String,
    },
    /// No published registry version satisfies the requirement.
    MissingRegistryVersion {
        /// Dependent package name.
        package: String,
        /// Dependency name.
        dep: String,
        /// Declared requirement.
        req: String,
    },
    /// A selected dependent needs an unpublished, unselected local package.
    UnpublishedLocalDep {
        /// Dependent package name.
        dependent: String,
        /// Missing dependency name.
        dep: String,
        /// Declared requirement.
        req: String,
    },
    /// A dependency source is not registry, path, or Git.
    UnsupportedSource {
        /// Dependent package name.
        package: String,
        /// Dependency name.
        dep: String,
        /// Source value found.
        source: String,
    },
    /// Normal/build/optional/target edges still cycle without dev edges.
    PublishCycle {
        /// Members stuck in the cycle.
        members: Vec<String>,
    },
    /// Supplementary facts disagree with the retained metadata graph.
    MetadataMismatch {
        /// Diagnostic detail.
        detail: String,
    },
    /// Emission or graph work was requested with no selected packages.
    NothingSelected,
    /// The tag pattern lacks placeholders or holds control characters.
    InvalidTagPattern {
        /// Offending pattern.
        pattern: String,
    },
    /// A version-group member is not in the release set.
    UnknownGroupMember {
        /// Group name.
        group: String,
        /// Offending member.
        member: String,
    },
    /// One package belongs to two version groups.
    OverlappingGroups {
        /// Repeated member.
        member: String,
    },
    /// A version group names no members.
    EmptyGroup {
        /// Group name.
        group: String,
    },
    /// A version-group name is malformed.
    InvalidGroupName {
        /// Offending name.
        group: String,
    },
    /// A version-group input version is not strict semver.
    InvalidGroupVersion {
        /// Group name.
        group: String,
        /// Member name.
        member: String,
        /// Offending version.
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
