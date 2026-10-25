use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::metadata::{PackageRecord, WorkspaceRecord, parse_metadata_json};
use crate::release_error::ReleaseError;
use crate::release_facts::{
    PublishSetting, ReleaseFacts, parse_release_facts, validate_manifest_path,
    validate_package_name, validate_registry_name,
};
use crate::release_semver::parse_version;

pub const DEFAULT_REGISTRY: &str = "crates-io";

pub const GIT_TAG_REGISTRY: &str = "git-tag";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseScope {
    Packages(Vec<String>),
    GitTagPackages(Vec<String>),
    PublishableWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedScope {
    Disabled,
    Explicit,
    PublishableWorkspace,
}

#[derive(Debug, Clone)]
pub struct ReleaseRequest<'a> {
    pub metadata_json: &'a str,
    pub repo_root: &'a Path,
    pub manifest_hint: &'a str,
    pub scope: &'a ReleaseScope,
    pub enabled: bool,
    pub affected: &'a BTreeSet<String>,
    pub supported_registries: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedPackage {
    pub id: String,
    pub name: String,
    pub version: String,
    pub manifest: String,
    pub registries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseSelection {
    pub packages: Vec<SelectedPackage>,
    pub scope: ResolvedScope,
    pub(crate) facts: ReleaseFacts,
}

impl ReleaseSelection {
    #[must_use]
    pub fn len(&self) -> usize {
        self.packages.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.packages
            .iter()
            .map(|package| package.name.as_str())
            .collect()
    }
}

pub(crate) fn supported_or_default(supported: &[String]) -> Vec<String> {
    if supported.is_empty() {
        vec![DEFAULT_REGISTRY.to_owned()]
    } else {
        supported.to_vec()
    }
}

pub fn select_release_set(request: &ReleaseRequest<'_>) -> Result<ReleaseSelection, ReleaseError> {
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
            select_explicit(names, &record, &members, &facts, Some(&supported))
        }
        ReleaseScope::GitTagPackages(names) => {
            select_explicit(names, &record, &members, &facts, None)
        }
        ReleaseScope::PublishableWorkspace => select_publishable(&members, &facts, &supported),
    }
}

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

fn select_explicit(
    names: &[String],
    record: &WorkspaceRecord,
    members: &BTreeMap<String, &PackageRecord>,
    facts: &ReleaseFacts,
    supported: Option<&[String]>,
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
        packages.push(match supported {
            Some(supported) => check_member(member, facts, supported)?,
            None => selected(member, vec![GIT_TAG_REGISTRY.to_owned()]),
        });
    }
    packages.sort_by(|left: &SelectedPackage, right: &SelectedPackage| left.name.cmp(&right.name));
    Ok(ReleaseSelection {
        packages,
        scope: ResolvedScope::Explicit,
        facts: facts.clone(),
    })
}

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

fn selected(member: &PackageRecord, registries: Vec<String>) -> SelectedPackage {
    SelectedPackage {
        id: member.id.clone(),
        name: member.name.clone(),
        version: member.version.clone(),
        manifest: member.manifest.clone(),
        registries,
    }
}
