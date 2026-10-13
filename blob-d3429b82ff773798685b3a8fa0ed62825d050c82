//! Release-set derivation from Cargo metadata plus declared policy.
//!
//! One mechanism serves a single root package, an explicit list, and the
//! explicit publishable-workspace opt-in: every mode resolves to an explicit
//! package allowlist validated against authoritative `cargo metadata`.
//! `publish = false` and registry restrictions always win over selection,
//! and CI affected-state never grants eligibility.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::metadata::{PackageRecord, WorkspaceRecord, parse_metadata_json};
use crate::release_error::ReleaseError;
use crate::release_facts::{
    PublishSetting, ReleaseFacts, parse_release_facts, validate_manifest_path,
    validate_package_name, validate_registry_name,
};
use crate::release_semver::parse_version;

/// Registry assumed when a package declares no restriction.
pub const DEFAULT_REGISTRY: &str = "crates-io";

/// Declared release scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseScope {
    /// Explicit package allowlist (one root package or a selected list).
    Packages(Vec<String>),
    /// Explicit opt-in: every publishable workspace member.
    PublishableWorkspace,
}

/// How selection resolved (receipt input).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedScope {
    /// Release support disabled; the set is always empty.
    Disabled,
    /// Resolved from an explicit allowlist.
    Explicit,
    /// Resolved from the publishable-workspace opt-in.
    PublishableWorkspace,
}

/// Selection inputs.
#[derive(Debug, Clone)]
pub struct ReleaseRequest<'a> {
    /// Authoritative `cargo metadata` JSON.
    pub metadata_json: &'a str,
    /// Canonical repository root.
    pub repo_root: &'a Path,
    /// Candidate manifest used in diagnostics.
    pub manifest_hint: &'a str,
    /// Declared scope.
    pub scope: &'a ReleaseScope,
    /// Whether release support is enabled.
    pub enabled: bool,
    /// CI affected-state: accepted for threading, never grants eligibility.
    pub affected: &'a BTreeSet<String>,
    /// Registries the pipeline can publish to (empty means default only).
    pub supported_registries: &'a [String],
}

/// One selected releasable package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedPackage {
    /// Opaque Cargo package id.
    pub id: String,
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// Repository-relative manifest path.
    pub manifest: String,
    /// Effective publishable registries (sorted, deduped).
    pub registries: Vec<String>,
}

/// Derived release set plus the facts its graph needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseSelection {
    /// Selected packages sorted by name.
    pub packages: Vec<SelectedPackage>,
    /// How the set resolved.
    pub scope: ResolvedScope,
    /// Supplementary facts for the publication graph.
    pub(crate) facts: ReleaseFacts,
}

impl ReleaseSelection {
    /// Number of selected packages.
    #[must_use]
    pub fn len(&self) -> usize {
        self.packages.len()
    }

    /// Whether no package was selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    /// Selected package names in order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.packages
            .iter()
            .map(|package| package.name.as_str())
            .collect()
    }
}

/// Effective supported registries (empty input means the default only).
pub(crate) fn supported_or_default(supported: &[String]) -> Vec<String> {
    if supported.is_empty() {
        vec![DEFAULT_REGISTRY.to_owned()]
    } else {
        supported.to_vec()
    }
}

/// Derive the release set from metadata plus declared policy.
///
/// # Errors
///
/// Returns [`ReleaseError`] on unknown, duplicate, forbidden, or otherwise
/// unverifiable selection; [`ReleaseError::Metadata`] on bad metadata.
pub fn select_release_set(request: &ReleaseRequest<'_>) -> Result<ReleaseSelection, ReleaseError> {
    // CI affected-state is threaded through for API stability but must never
    // widen the authorized upload set; selection consults policy only.
    let _ = request.affected;
    if !request.enabled {
        return Ok(ReleaseSelection {
            packages: Vec::new(),
            scope: ResolvedScope::Disabled,
            facts: ReleaseFacts {
                publish: BTreeMap::new(),
                deps: BTreeMap::new(),
            },
        });
    }
    // Release resolves one workspace without the discovery candidate list,
    // so every unresolvable path edge fails closed here, as before.
    let record = parse_metadata_json(
        request.metadata_json,
        request.repo_root,
        request.manifest_hint,
        &BTreeSet::new(),
    )
    .map_err(ReleaseError::Metadata)?;
    let facts = parse_release_facts(request.metadata_json, request.manifest_hint)?;
    let members = index_members(&record)?;
    let supported = supported_or_default(request.supported_registries);
    for registry in &supported {
        if !validate_registry_name(registry) {
            return Err(ReleaseError::InvalidRegistry {
                package: String::new(),
                registry: registry.clone(),
            });
        }
    }
    match request.scope {
        ReleaseScope::Packages(names) => {
            select_explicit(names, &record, &members, &facts, &supported)
        }
        ReleaseScope::PublishableWorkspace => select_publishable(&members, &facts, &supported),
    }
}

/// Index workspace members by name after validating every member record.
fn index_members(
    record: &WorkspaceRecord,
) -> Result<BTreeMap<String, &PackageRecord>, ReleaseError> {
    let mut members = BTreeMap::new();
    for package in &record.packages {
        if !package.in_workspace {
            continue;
        }
        if !validate_package_name(&package.name) {
            return Err(ReleaseError::InvalidPackageName {
                name: package.name.clone(),
            });
        }
        if parse_version(&package.version).is_none() {
            return Err(ReleaseError::InvalidVersion {
                package: package.name.clone(),
                version: package.version.clone(),
            });
        }
        if package.external {
            return Err(ReleaseError::ManifestOutsideRoot {
                package: package.name.clone(),
                manifest: package.manifest.clone(),
            });
        }
        if !validate_manifest_path(&package.manifest) {
            return Err(ReleaseError::ManifestEscape {
                package: package.name.clone(),
                manifest: package.manifest.clone(),
            });
        }
        if members.insert(package.name.clone(), package).is_some() {
            return Err(ReleaseError::AmbiguousPackageName {
                name: package.name.clone(),
            });
        }
    }
    Ok(members)
}

/// Resolve an explicit allowlist (duplicates and unknowns fail closed).
fn select_explicit(
    names: &[String],
    record: &WorkspaceRecord,
    members: &BTreeMap<String, &PackageRecord>,
    facts: &ReleaseFacts,
    supported: &[String],
) -> Result<ReleaseSelection, ReleaseError> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !validate_package_name(name) {
            return Err(ReleaseError::InvalidSelectionName { name: name.clone() });
        }
        if !seen.insert(name.clone()) {
            return Err(ReleaseError::DuplicateSelection { name: name.clone() });
        }
    }
    let mut packages = Vec::with_capacity(names.len());
    for name in names {
        let Some(member) = members.get(name) else {
            return Err(member_error(record, name));
        };
        packages.push(check_member(member, facts, supported)?);
    }
    packages.sort_by(|left: &SelectedPackage, right: &SelectedPackage| left.name.cmp(&right.name));
    Ok(ReleaseSelection {
        packages,
        scope: ResolvedScope::Explicit,
        facts: facts.clone(),
    })
}

/// Distinguish an external known name from a truly unknown one.
fn member_error(record: &WorkspaceRecord, name: &str) -> ReleaseError {
    let external = record
        .packages
        .iter()
        .any(|package| !package.in_workspace && package.name == name);
    if external {
        ReleaseError::NotWorkspaceMember {
            name: name.to_owned(),
        }
    } else {
        ReleaseError::UnknownPackage {
            name: name.to_owned(),
        }
    }
}

/// Resolve the publishable-workspace opt-in (`publish = false` skipped).
fn select_publishable(
    members: &BTreeMap<String, &PackageRecord>,
    facts: &ReleaseFacts,
    supported: &[String],
) -> Result<ReleaseSelection, ReleaseError> {
    let mut packages = Vec::new();
    for member in members.values() {
        if facts.publish.get(&member.id) == Some(&PublishSetting::Forbidden) {
            continue;
        }
        packages.push(check_member(member, facts, supported)?);
    }
    packages.sort_by(|left: &SelectedPackage, right: &SelectedPackage| left.name.cmp(&right.name));
    Ok(ReleaseSelection {
        packages,
        scope: ResolvedScope::PublishableWorkspace,
        facts: facts.clone(),
    })
}

/// Check one member against its `publish` state and registry restrictions.
fn check_member(
    member: &PackageRecord,
    facts: &ReleaseFacts,
    supported: &[String],
) -> Result<SelectedPackage, ReleaseError> {
    let setting = facts
        .publish
        .get(&member.id)
        .ok_or_else(|| ReleaseError::MetadataMismatch {
            detail: format!("missing publish facts for {}", member.name),
        })?;
    match setting {
        PublishSetting::Forbidden => Err(ReleaseError::PublishForbidden {
            package: member.name.clone(),
        }),
        PublishSetting::Open => {
            if !supported.contains(&DEFAULT_REGISTRY.to_owned()) {
                return Err(ReleaseError::UnsupportedRegistry {
                    package: member.name.clone(),
                    registry: DEFAULT_REGISTRY.to_owned(),
                });
            }
            Ok(selected(member, vec![DEFAULT_REGISTRY.to_owned()]))
        }
        PublishSetting::Registries(registries) => {
            for registry in registries {
                if !supported.contains(registry) {
                    return Err(ReleaseError::UnsupportedRegistry {
                        package: member.name.clone(),
                        registry: registry.clone(),
                    });
                }
            }
            Ok(selected(member, registries.clone()))
        }
    }
}

/// Build one selected package record.
fn selected(member: &PackageRecord, registries: Vec<String>) -> SelectedPackage {
    SelectedPackage {
        id: member.id.clone(),
        name: member.name.clone(),
        version: member.version.clone(),
        manifest: member.manifest.clone(),
        registries,
    }
}
