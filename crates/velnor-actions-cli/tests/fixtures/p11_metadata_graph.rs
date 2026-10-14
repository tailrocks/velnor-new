//! Effective edition, MSRV, and locked-graph checks.

use std::error::Error;

use super::{Json, WORKSPACE_MANIFESTS, metadata_for, workspace_packages};

#[test]
fn metadata_effective_edition_is_2024() -> Result<(), Box<dyn Error>> {
    for manifest in WORKSPACE_MANIFESTS {
        for package in workspace_packages(&metadata_for(manifest)?)? {
            let name = package.get("name").and_then(Json::as_str).unwrap_or("?");
            let edition = package.get("edition").and_then(Json::as_str);
            assert_eq!(
                edition,
                Some("2024"),
                "{manifest}: {name} effective edition"
            );
        }
    }
    Ok(())
}

#[test]
fn metadata_msrv_resolves() -> Result<(), Box<dyn Error>> {
    for manifest in WORKSPACE_MANIFESTS {
        for package in workspace_packages(&metadata_for(manifest)?)? {
            let name = package.get("name").and_then(Json::as_str).unwrap_or("?");
            let rust_version = package.get("rust_version").and_then(Json::as_str);
            assert_eq!(
                rust_version,
                Some("1.99"),
                "{manifest}: {name} effective rust-version"
            );
        }
    }
    Ok(())
}

#[test]
fn metadata_locked_graph_is_registry_only() -> Result<(), Box<dyn Error>> {
    for manifest in WORKSPACE_MANIFESTS {
        assert_locked_graph_is_registry_only(&metadata_for(manifest)?, manifest)?;
    }
    Ok(())
}

fn assert_locked_graph_is_registry_only(doc: &Json, manifest: &str) -> Result<(), Box<dyn Error>> {
    const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";
    let resolve = doc
        .get("resolve")
        .ok_or_else(|| format!("no resolve graph in {manifest}: {doc:?}"))?;
    let nodes = resolve
        .get("nodes")
        .and_then(Json::as_arr)
        .ok_or("no nodes")?;
    assert!(!nodes.is_empty(), "{manifest}: empty locked graph");
    for node in nodes {
        let id = node.get("id").and_then(Json::as_str).unwrap_or("?");
        assert!(!id.contains("git+"), "{manifest}: {id} resolves from git");
    }
    for package in workspace_packages(doc)? {
        let name = package.get("name").and_then(Json::as_str).unwrap_or("?");
        let deps = package
            .get("dependencies")
            .and_then(Json::as_arr)
            .ok_or("no dependencies")?;
        for dep in deps {
            let source = dep.get("source").and_then(Json::as_str);
            let req = dep.get("req").and_then(Json::as_str).unwrap_or("?");
            assert!(req.starts_with('='), "{manifest}: {name} inexact req {req}");
            if let Some(source) = source {
                assert_eq!(source, REGISTRY, "{manifest}: {name} non-registry source");
            }
        }
    }
    Ok(())
}
