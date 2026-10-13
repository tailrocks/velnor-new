//! Public declaration consistency checks. No version/source/feature resolution.
use super::{ManifestDependency, Metadata, ObservationError, Package};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn validate(metadata: &Metadata) -> Result<(), ObservationError> {
    let packages: BTreeMap<_, _> = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect();
    for node in &metadata.resolve.nodes {
        let package = packages
            .get(node.id.as_str())
            .ok_or(ObservationError::InvalidGraph)?;
        for edge in &node.deps {
            let target = packages
                .get(edge.pkg.as_str())
                .ok_or(ObservationError::InvalidGraph)?;
            for kind in &edge.dep_kinds {
                if !package.dependencies.iter().any(|declared| {
                    declared.name == target.name
                        && edge_name(declared, target).as_deref() == Some(edge.name.as_str())
                        && declared.kind == kind.kind
                        && declared.target == kind.target
                }) {
                    return Err(ObservationError::InvalidGraph);
                }
            }
        }
        // A missing unconditional required normal/build edge is an unsupported
        // shape here. Optional, target-conditioned and dev activation stay unqualified.
        for declared in &package.dependencies {
            if declared.optional
                || !declared.target.is_null()
                || matches!(&declared.kind, Value::String(kind) if kind == "dev")
            {
                continue;
            }
            if !node.deps.iter().any(|edge| {
                packages.get(edge.pkg.as_str()).is_some_and(|target| {
                    target.name == declared.name
                        && edge_name(declared, target).as_deref() == Some(edge.name.as_str())
                }) && edge
                    .dep_kinds
                    .iter()
                    .any(|kind| kind.kind == declared.kind && kind.target.is_null())
            }) {
                return Err(ObservationError::InvalidGraph);
            }
        }
    }
    Ok(())
}

fn edge_name(declared: &ManifestDependency, package: &Package) -> Option<String> {
    if let Some(rename) = declared.rename.as_str() {
        return Some(rename.replace('-', "_"));
    }
    let mut libraries = package.targets.iter().filter(|target| {
        target.kind.iter().any(|kind| {
            matches!(
                kind.as_str(),
                "lib" | "rlib" | "dylib" | "staticlib" | "cdylib" | "proc-macro"
            )
        })
    });
    let target = libraries.next()?;
    if libraries.next().is_some() {
        return None;
    }
    Some(target.name.replace('-', "_"))
}
