//! Nested-workspace tolerance: termpane-shaped fixtures.
//!
//! A nested `[workspace]` root with a path dependency on its parent (or
//! the reverse: a root member depending on a nested workspace) is
//! definitionally outside the parsed workspace. `cargo metadata --no-deps`
//! omits such targets from `packages`; discovery must skip and record the
//! edge instead of failing the whole inventory. Dangling targets still
//! fail closed.
use std::collections::BTreeSet;
use std::path::Path;

use serde_json::json;

use crate::support::{Outcome, TempDir};
use velnor_actions_rust_core::{DepKind, MetadataError, parse_metadata_json};

/// Absolute manifest path text for `relative` under `root`.
fn manifest(root: &Path, relative: &str) -> String {
    root.join(relative).to_string_lossy().into_owned()
}

/// One package entry with `dependencies` as given.
fn package(
    root: &Path,
    name: &str,
    id: &str,
    manifest_rel: &str,
    dependencies: &[serde_json::Value],
) -> serde_json::Value {
    json!({
        "name": name,
        "version": "0.1.0",
        "id": id,
        "manifest_path": manifest(root, manifest_rel),
        "targets": [
            {"kind": ["lib"], "name": name, "test": true,
             "doctest": true, "required_features": []},
        ],
        "features": {},
        "dependencies": dependencies,
    })
}

/// One path-dep declaration on the absolute directory `path`.
fn path_dep(path: &str, kind: Option<&str>) -> serde_json::Value {
    json!({
        "name": "dep",
        "kind": kind,
        "optional": false,
        "target": null,
        "path": path,
    })
}

/// One metadata document rooted at `workspace_rel` with `packages`.
fn document(
    root: &Path,
    workspace_rel: &str,
    members: &[&str],
    packages: &[serde_json::Value],
) -> serde_json::Value {
    let workspace_root = if workspace_rel.is_empty() {
        root.to_string_lossy().into_owned()
    } else {
        root.join(workspace_rel).to_string_lossy().into_owned()
    };
    json!({
        "version": 1,
        "workspace_root": workspace_root,
        "workspace_members": members,
        "packages": packages,
    })
}

/// Known-manifest set for the termpane shape.
fn termpane_known() -> BTreeSet<String> {
    BTreeSet::from(["Cargo.toml".to_owned(), "fuzz/Cargo.toml".to_owned()])
}

/// Fuzz-workspace document: `fuzz` depends on its parent by path.
fn fuzz_document(root: &Path) -> serde_json::Value {
    let parent = root.to_string_lossy().into_owned();
    document(
        root,
        "fuzz",
        &["fuzz-id"],
        &[package(
            root,
            "fuzz",
            "fuzz-id",
            "fuzz/Cargo.toml",
            &[path_dep(&parent, None)],
        )],
    )
}

#[test]
fn nested_workspace_path_dep_on_parent_skips_and_records() -> Outcome {
    let dir = TempDir::create("nested-fuzz")?;
    let root = dir.path().canonicalize()?;
    let known = termpane_known();
    let record = parse_metadata_json(
        &fuzz_document(&root).to_string(),
        &root,
        "fuzz/Cargo.toml",
        &known,
    )?;
    assert_eq!(record.workspace_root, "fuzz");
    assert_eq!(record.members, vec!["fuzz-id".to_owned()]);
    assert_eq!(record.packages.len(), 1);
    assert!(record.edges.is_empty());
    assert_eq!(record.skipped_edges.len(), 1);
    let skipped = &record.skipped_edges[0];
    assert_eq!(skipped.from, "fuzz-id");
    assert_eq!(skipped.path, root.to_string_lossy());
    assert_eq!(skipped.kind, DepKind::Normal);
    assert!(!skipped.optional);
    Ok(())
}

#[test]
fn root_document_omitting_nested_workspace_parses_cleanly() -> Outcome {
    let dir = TempDir::create("nested-root")?;
    let root = dir.path().canonicalize()?;
    let known = termpane_known();
    let doc = document(
        &root,
        "",
        &["termpane-id"],
        &[package(&root, "termpane", "termpane-id", "Cargo.toml", &[])],
    );
    let record = parse_metadata_json(&doc.to_string(), &root, "Cargo.toml", &known)?;
    assert_eq!(record.workspace_root, "");
    assert_eq!(record.members, vec!["termpane-id".to_owned()]);
    assert!(record.edges.is_empty());
    assert!(record.skipped_edges.is_empty());
    Ok(())
}

#[test]
fn dangling_target_still_fails_with_populated_known_set() -> Outcome {
    let dir = TempDir::create("nested-dangling")?;
    let root = dir.path().canonicalize()?;
    let known = termpane_known();
    let ghost = root.join("crates/ghost").to_string_lossy().into_owned();
    let doc = document(
        &root,
        "fuzz",
        &["fuzz-id"],
        &[package(
            &root,
            "fuzz",
            "fuzz-id",
            "fuzz/Cargo.toml",
            &[path_dep(&ghost, None)],
        )],
    );
    let result = parse_metadata_json(&doc.to_string(), &root, "fuzz/Cargo.toml", &known);
    assert!(matches!(
        result,
        Err(MetadataError::UnresolvedPathEdge { .. })
    ));
    Ok(())
}

#[test]
fn unlisted_target_still_fails() -> Outcome {
    let dir = TempDir::create("nested-unlisted")?;
    let root = dir.path().canonicalize()?;
    // The parent manifest is not among the known manifests (e.g. it was
    // never discovered), so the edge cannot be verified as cross-workspace.
    let known = BTreeSet::from(["fuzz/Cargo.toml".to_owned()]);
    let result = parse_metadata_json(
        &fuzz_document(&root).to_string(),
        &root,
        "fuzz/Cargo.toml",
        &known,
    );
    assert!(matches!(
        result,
        Err(MetadataError::UnresolvedPathEdge { .. })
    ));
    Ok(())
}

#[test]
fn skip_path_rejects_unknown_dep_kinds() -> Outcome {
    let dir = TempDir::create("nested-kind")?;
    let root = dir.path().canonicalize()?;
    let known = termpane_known();
    let parent = root.to_string_lossy().into_owned();
    let doc = document(
        &root,
        "fuzz",
        &["fuzz-id"],
        &[package(
            &root,
            "fuzz",
            "fuzz-id",
            "fuzz/Cargo.toml",
            &[path_dep(&parent, Some("mystery"))],
        )],
    );
    let result = parse_metadata_json(&doc.to_string(), &root, "fuzz/Cargo.toml", &known);
    assert!(matches!(result, Err(MetadataError::UnknownDepKind { .. })));
    Ok(())
}

#[test]
fn root_member_edge_to_nested_workspace_skips() -> Outcome {
    let dir = TempDir::create("nested-reverse")?;
    let root = dir.path().canonicalize()?;
    let known = BTreeSet::from([
        "Cargo.toml".to_owned(),
        "crates/a/Cargo.toml".to_owned(),
        "nested/lib/Cargo.toml".to_owned(),
    ]);
    let lib = root.join("nested/lib").to_string_lossy().into_owned();
    let doc = document(
        &root,
        "",
        &["a-id"],
        &[package(
            &root,
            "a",
            "a-id",
            "crates/a/Cargo.toml",
            &[path_dep(&lib, Some("dev"))],
        )],
    );
    let record = parse_metadata_json(&doc.to_string(), &root, "Cargo.toml", &known)?;
    assert_eq!(record.members, vec!["a-id".to_owned()]);
    assert!(record.edges.is_empty());
    assert_eq!(record.skipped_edges.len(), 1);
    assert_eq!(record.skipped_edges[0].kind, DepKind::Dev);
    Ok(())
}

/// Termpane-shaped fixture: root package plus a nested `fuzz` workspace
/// with a path dependency on its parent.
fn write_termpane_fixture(dir: &TempDir) -> std::io::Result<()> {
    dir.write(
        "Cargo.toml",
        "[package]\nname = \"termpane\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\nexclude = [\"fuzz\"]\n",
    )?;
    dir.write("src/lib.rs", "pub fn f() {}\n")?;
    dir.write(
        "fuzz/Cargo.toml",
        "[package]\nname = \"fuzz\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n\n[dependencies]\ntermpane = { path = \"..\" }\n",
    )?;
    dir.write("fuzz/src/lib.rs", "pub fn f() {}\n")?;
    Ok(())
}

/// Real `cargo metadata --no-deps --offline` output for one manifest.
fn run_metadata(manifest: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed for {}", manifest.display()).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[test]
fn real_cargo_termpane_shape_parses_both_workspaces() -> Outcome {
    let dir = TempDir::create("nested-real")?;
    write_termpane_fixture(&dir)?;
    let root = dir.path().canonicalize()?;
    let known = termpane_known();
    let root_json = run_metadata(&root.join("Cargo.toml"))?;
    let fuzz_json = run_metadata(&root.join("fuzz/Cargo.toml"))?;
    let root_record = parse_metadata_json(&root_json, &root, "Cargo.toml", &known)?;
    assert_eq!(root_record.members.len(), 1);
    assert!(root_record.skipped_edges.is_empty());
    let fuzz_record = parse_metadata_json(&fuzz_json, &root, "fuzz/Cargo.toml", &known)?;
    assert_eq!(fuzz_record.workspace_root, "fuzz");
    assert_eq!(fuzz_record.members.len(), 1);
    assert!(fuzz_record.edges.is_empty());
    assert_eq!(fuzz_record.skipped_edges.len(), 1);
    // The same real fuzz document stays fail-closed without the known set.
    let empty = BTreeSet::new();
    let strict = parse_metadata_json(&fuzz_json, &root, "fuzz/Cargo.toml", &empty);
    assert!(matches!(
        strict,
        Err(MetadataError::UnresolvedPathEdge { .. })
    ));
    Ok(())
}
