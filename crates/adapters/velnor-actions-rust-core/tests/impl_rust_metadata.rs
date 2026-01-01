//! Metadata parsing and local-graph cases.
use std::collections::BTreeSet;
use std::path::Path;

use serde_json::json;

use crate::support::{Outcome, TempDir};
use velnor_actions_contract_planning::reverse_closure;
use velnor_actions_rust_core::{
    DepKind, LocalEdge, MetadataError, dedupe_workspaces, local_edge_pairs, parse_metadata_json,
};

/// Absolute manifest path text for `relative` under `root`.
fn manifest(root: &Path, relative: &str) -> String {
    root.join(relative).to_string_lossy().into_owned()
}

/// Sample `cargo metadata` with normal, optional, target, dev, and build edges.
fn sample_metadata(root: &Path) -> serde_json::Value {
    let root_text = root.to_string_lossy().into_owned();
    json!({
        "version": 1,
        "workspace_root": root_text,
        "target_directory": format!("{root_text}/target"),
        "workspace_members": ["a-id", "b-id"],
        "resolve": null,
        "packages": [
            {
                "name": "a",
                "version": "0.1.0",
                "id": "a-id",
                "manifest_path": manifest(root, "crates/a/Cargo.toml"),
                "targets": [
                    {"kind": ["lib"], "name": "a", "src_path": "src/lib.rs",
                     "test": true, "doctest": true, "required_features": []},
                    {"kind": ["bin"], "name": "a-bin", "src_path": "src/main.rs",
                     "test": true, "doctest": false, "required_features": []},
                ],
                "features": {"default": [], "extra": []},
                "dependencies": [],
            },
            {
                "name": "b",
                "version": "0.2.0",
                "id": "b-id",
                "manifest_path": manifest(root, "crates/b/Cargo.toml"),
                "targets": [
                    {"kind": ["lib"], "name": "b", "src_path": "src/lib.rs",
                     "test": true, "doctest": false, "required_features": []},
                    {"kind": ["custom-build"], "name": "build-script-build",
                     "src_path": "build.rs", "test": false, "doctest": false,
                     "required_features": []},
                ],
                "features": {"default": []},
                "dependencies": [
                    {"name": "a", "kind": null, "optional": false, "target": null,
                     "path": format!("{root_text}/crates/a")},
                    {"name": "opt-a", "kind": null, "optional": true, "target": null,
                     "path": format!("{root_text}/crates/a")},
                    {"name": "win-a", "kind": null, "optional": false,
                     "target": "cfg(windows)", "path": format!("{root_text}/crates/a")},
                    {"name": "dev-a", "kind": "dev", "optional": false, "target": null,
                     "path": format!("{root_text}/crates/a")},
                    {"name": "build-a", "kind": "build", "optional": false, "target": null,
                     "path": format!("{root_text}/crates/a")},
                    {"name": "serde", "kind": null, "optional": false, "target": null,
                     "path": null},
                ],
            },
        ],
    })
}

#[test]
fn parses_no_deps_metadata_into_conservative_graph() -> Outcome {
    let dir = TempDir::create("meta-graph")?;
    let root = dir.path().canonicalize()?;
    // A populated known-manifest set must never divert intra-workspace edges.
    let known = BTreeSet::from([
        "Cargo.toml".to_owned(),
        "crates/a/Cargo.toml".to_owned(),
        "crates/b/Cargo.toml".to_owned(),
    ]);
    let record = parse_metadata_json(
        &sample_metadata(&root).to_string(),
        &root,
        "Cargo.toml",
        &known,
    )?;
    assert_eq!(record.workspace_root, "");
    assert_eq!(record.members, vec!["a-id".to_owned(), "b-id".to_owned()]);
    assert_eq!(record.packages.len(), 2);
    assert!(record.packages.iter().all(|package| package.in_workspace));
    assert_eq!(record.edges.len(), 5);
    assert!(record.edges.iter().any(|edge| edge.optional));
    assert!(
        record
            .edges
            .iter()
            .any(|edge| edge.target.as_deref() == Some("cfg(windows)"))
    );
    assert!(record.edges.iter().any(|edge| edge.kind == DepKind::Dev));
    assert!(record.edges.iter().any(|edge| edge.kind == DepKind::Build));
    assert!(record.edges.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(record.skipped_edges.is_empty());
    Ok(())
}

#[test]
fn records_targets_features_doctests_and_build_scripts() -> Outcome {
    let dir = TempDir::create("meta-inventory")?;
    let root = dir.path().canonicalize()?;
    let known = BTreeSet::new();
    let record = parse_metadata_json(
        &sample_metadata(&root).to_string(),
        &root,
        "Cargo.toml",
        &known,
    )?;
    let package_a = record.packages.iter().find(|p| p.id == "a-id");
    let Some(package_a) = package_a else {
        return Err("package a-id must be retained".into());
    };
    assert_eq!(package_a.name, "a");
    assert_eq!(package_a.version, "0.1.0");
    assert_eq!(package_a.manifest, "crates/a/Cargo.toml");
    assert!(!package_a.external);
    assert!(!package_a.has_build_script);
    assert_eq!(
        package_a.features,
        vec!["default".to_owned(), "extra".to_owned()]
    );
    assert!(
        package_a
            .targets
            .iter()
            .any(|t| t.kind == "lib" && t.doctest)
    );
    let package_b = record.packages.iter().find(|p| p.id == "b-id");
    let Some(package_b) = package_b else {
        return Err("package b-id must be retained".into());
    };
    assert!(package_b.has_build_script);
    assert!(package_b.targets.iter().any(|t| t.kind == "custom-build"));
    Ok(())
}

#[test]
fn rejects_invalid_json_and_bad_version_without_fallback() -> Outcome {
    let dir = TempDir::create("meta-invalid")?;
    let root = dir.path().canonicalize()?;
    let known = BTreeSet::new();
    let bad = parse_metadata_json("not json", &root, "Cargo.toml", &known);
    assert!(matches!(bad, Err(MetadataError::InvalidJson { .. })));
    let mut document = sample_metadata(&root);
    document["version"] = json!(2);
    let versioned = parse_metadata_json(&document.to_string(), &root, "Cargo.toml", &known);
    assert!(matches!(
        versioned,
        Err(MetadataError::UnsupportedVersion { found: 2, .. })
    ));
    let mut missing = sample_metadata(&root);
    missing["packages"][0]
        .as_object_mut()
        .map(|map| map.remove("id"));
    let incomplete = parse_metadata_json(&missing.to_string(), &root, "Cargo.toml", &known);
    assert!(matches!(incomplete, Err(MetadataError::InvalidJson { .. })));
    Ok(())
}

#[test]
fn rejects_unmapped_edges_and_unknown_kinds() -> Outcome {
    let dir = TempDir::create("meta-edges")?;
    let root = dir.path().canonicalize()?;
    let known = BTreeSet::new();
    let mut dangling = sample_metadata(&root);
    dangling["packages"][1]["dependencies"][0]["path"] =
        json!(format!("{}/crates/missing", root.to_string_lossy()));
    let result = parse_metadata_json(&dangling.to_string(), &root, "Cargo.toml", &known);
    assert!(matches!(
        result,
        Err(MetadataError::UnresolvedPathEdge { .. })
    ));
    let mut kinded = sample_metadata(&root);
    kinded["packages"][1]["dependencies"][0]["kind"] = json!("mystery");
    let result = parse_metadata_json(&kinded.to_string(), &root, "Cargo.toml", &known);
    assert!(matches!(result, Err(MetadataError::UnknownDepKind { .. })));
    Ok(())
}

#[test]
fn resolves_duplicate_names_by_id_and_manifest() -> Outcome {
    let dir = TempDir::create("meta-dups")?;
    let root = dir.path().canonicalize()?;
    let known = BTreeSet::new();
    let mut document = sample_metadata(&root);
    document["packages"][1]["name"] = json!("a");
    let record = parse_metadata_json(&document.to_string(), &root, "Cargo.toml", &known)?;
    let names: Vec<&str> = record
        .packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    assert_eq!(names, vec!["a", "a"]);
    let ids: Vec<&str> = record
        .packages
        .iter()
        .map(|package| package.id.as_str())
        .collect();
    assert_eq!(ids, vec!["a-id", "b-id"]);
    assert_ne!(record.packages[0].manifest, record.packages[1].manifest);
    Ok(())
}

#[test]
fn flags_path_dependencies_outside_the_root() -> Outcome {
    let dir = TempDir::create("meta-external")?;
    let other = TempDir::create("meta-external-other")?;
    let root = dir.path().canonicalize()?;
    let outside = other.path().canonicalize()?;
    let mut document = sample_metadata(&root);
    let outside_manifest = manifest(&outside, "ext/Cargo.toml");
    document["packages"][0]["manifest_path"] = json!(outside_manifest.clone());
    document["packages"][1]["dependencies"][0]["path"] =
        json!(outside.join("ext").to_string_lossy().into_owned());
    for index in 1..5 {
        document["packages"][1]["dependencies"][index]["path"] = json!(null);
    }
    let known = BTreeSet::new();
    let record = parse_metadata_json(&document.to_string(), &root, "Cargo.toml", &known)?;
    let package_a = record.packages.iter().find(|p| p.id == "a-id");
    let Some(package_a) = package_a else {
        return Err("package a-id must be retained".into());
    };
    assert!(package_a.external);
    assert_eq!(package_a.manifest, outside_manifest.replace('\\', "/"));
    assert_eq!(record.edges.len(), 1);
    Ok(())
}

#[test]
fn reverse_closure_unions_base_and_head() {
    let edge = |from: &str, to: &str| LocalEdge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind: DepKind::Normal,
        optional: false,
        target: None,
    };
    let base = vec![edge("b", "a"), edge("c", "b")];
    let head = vec![edge("c", "b")];
    let changed = BTreeSet::from(["a".to_owned()]);
    let base_pairs = local_edge_pairs(&base);
    let head_pairs = local_edge_pairs(&head);
    let selected = reverse_closure(&base_pairs, &head_pairs, &changed);
    assert_eq!(
        selected,
        BTreeSet::from(["a".to_owned(), "b".to_owned(), "c".to_owned()])
    );
    let changed_leaf = BTreeSet::from(["c".to_owned()]);
    assert_eq!(
        reverse_closure(&base_pairs, &head_pairs, &changed_leaf),
        changed_leaf
    );
}

#[test]
fn chain_selects_transitive_consumers() {
    let edge = |from: &str, to: &str| LocalEdge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind: DepKind::Normal,
        optional: false,
        target: None,
    };
    let chain = vec![edge("b", "a"), edge("c", "b"), edge("d", "c")];
    let changed = BTreeSet::from(["a".to_owned()]);
    let chain_pairs = local_edge_pairs(&chain);
    assert_eq!(
        reverse_closure(&chain_pairs, &chain_pairs, &changed),
        BTreeSet::from([
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "d".to_owned(),
        ])
    );
    let renamed_base = vec![edge("b", "old-a")];
    let renamed_head = vec![edge("b", "new-a")];
    let renamed = BTreeSet::from(["old-a".to_owned(), "new-a".to_owned()]);
    let renamed_base_pairs = local_edge_pairs(&renamed_base);
    let renamed_head_pairs = local_edge_pairs(&renamed_head);
    assert_eq!(
        reverse_closure(&renamed_base_pairs, &renamed_head_pairs, &renamed),
        BTreeSet::from(["b".to_owned(), "new-a".to_owned(), "old-a".to_owned()]),
        "renamed edges select consumers from both graphs"
    );
}

#[test]
fn dedupe_workspaces_by_root() -> Outcome {
    let dir = TempDir::create("meta-dedupe")?;
    let root = dir.path().canonicalize()?;
    let json = sample_metadata(&root).to_string();
    let known = BTreeSet::new();
    let first = parse_metadata_json(&json, &root, "Cargo.toml", &known)?;
    let second = parse_metadata_json(&json, &root, "crates/a/Cargo.toml", &known)?;
    assert_eq!(dedupe_workspaces(vec![first, second]).len(), 1);
    Ok(())
}
