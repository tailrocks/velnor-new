//! Registry-independent obligations of the authorized local release set.
//!
//! Selection alone proves eligibility. Generation must also prove selected
//! local versions satisfy their declarations before freezing publisher inputs.

use std::collections::{BTreeMap, BTreeSet};

use crate::metadata_edges::DepKind;
use crate::release_error::ReleaseError;
use crate::release_facts::{DepFact, DepSource};
use crate::release_select::ReleaseSelection;
use crate::release_semver::{VersionReq, parse_req, parse_version, req_matches};

use super::{PackagingEdge, topo_order};

/// Validate selected local requirements and packaging cycles at generation.
///
/// This pure check uses the selected versions, regardless of other versions
/// already published. External and unselected dependencies remain obligations
/// of anonymous Cargo packaging and source index verification.
///
/// # Errors
///
/// Returns [`ReleaseError`] for an invalid selected local requirement, a
/// selected version mismatch, inconsistent facts, or a packaging cycle.
pub fn validate_selected_dependency_obligations(
    selection: &ReleaseSelection,
) -> Result<(), ReleaseError> {
    if selection.is_empty() {
        return Err(ReleaseError::NothingSelected);
    }
    let by_id: BTreeMap<_, _> = selection
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect();
    let names: BTreeSet<_> = selection
        .packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    let mut edges = Vec::new();
    for package in &selection.packages {
        let facts = selection.facts.deps.get(&package.id).ok_or_else(|| {
            ReleaseError::MetadataMismatch {
                detail: format!("missing dependency facts for {}", package.name),
            }
        })?;
        for fact in facts {
            if fact.source != DepSource::Path {
                continue;
            }
            let to = fact
                .to
                .as_deref()
                .ok_or_else(|| ReleaseError::MetadataMismatch {
                    detail: format!("path dep {} of {} has no target", fact.name, package.name),
                })?;
            let Some(target) = by_id.get(to) else {
                continue;
            };
            let req = parse_req(&fact.req).ok_or_else(|| ReleaseError::InvalidRequirement {
                package: package.name.clone(),
                dep: fact.name.clone(),
                req: fact.req.clone(),
            })?;
            check_path_version(&package.name, fact, &target.name, &target.version, &req)?;
            if fact.kind != DepKind::Dev {
                edges.push(PackagingEdge {
                    from: package.name.clone(),
                    to: target.name.clone(),
                    kind: fact.kind,
                    optional: fact.optional,
                    target: fact.target.clone(),
                });
            }
        }
    }
    topo_order(&names, &edges).map(|_| ())
}

/// Shared local version check for observed-registry and generation graphs.
pub(super) fn check_path_version(
    name: &str,
    fact: &DepFact,
    target: &str,
    version: &str,
    req: &VersionReq,
) -> Result<(), ReleaseError> {
    let parsed = parse_version(version).ok_or_else(|| ReleaseError::InvalidVersion {
        package: target.to_owned(),
        version: version.to_owned(),
    })?;
    if req_matches(req, &parsed) {
        Ok(())
    } else {
        Err(ReleaseError::RequirementMismatch {
            package: name.to_owned(),
            dep: fact.name.clone(),
            req: fact.req.clone(),
            found: version.to_owned(),
        })
    }
}
