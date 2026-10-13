//! Base/head dependency-graph edges for affected-work selection.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;
use velnor_actions_rust::{DepKind, LocalEdge};

use crate::discover::Discovery;
use crate::select_affected::manifest_dir;
use crate::validators::{validate_diff_rev, validate_select_diff_args, validate_select_show_args};

/// Max base manifests fetched; beyond this, broaden instead of reading.
const MAX_BASE_MANIFEST_BATCH: usize = 512;

/// Head paths added since base: manifests absent at base, skipped by batch.
///
/// Rename detection stays off so every head path missing at base reports as
/// added; the changed set keeps its own rename behavior untouched.
fn added_files(root: &Path, base: &str, head: &str) -> Result<BTreeSet<String>, String> {
    validate_diff_rev(base, "bad_base")?;
    validate_diff_rev(head, "bad_head")?;
    let range = format!("{base}...{head}");
    let args = vec![
        OsString::from("--name-only"),
        OsString::from("--no-renames"),
        OsString::from("--diff-filter=A"),
        OsString::from(range),
        OsString::from("--"),
    ];
    validate_select_diff_args(&args).map_err(|err| err.to_string())?;
    let output = GitRequest::diff(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    let text = output.stdout_text("git").map_err(|err| err.to_string())?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Head local-path edges from discovery records.
pub(crate) fn head_edges(discovery: &Discovery) -> Vec<LocalEdge> {
    let mut edges = Vec::new();
    for workspace in &discovery.workspaces {
        edges.extend(workspace.record.edges.iter().cloned());
    }
    edges
}

/// Base local-path edges from base-revision manifests, in head-id space.
///
/// Every wanted manifest is read at `base` with one `git show` each; path
/// dependencies resolve to the head package owning the target directory, so
/// removed or renamed edges still select their head consumers. Manifests
/// added since base are new and contribute nothing; over-cap sets and
/// fetch or parse failures are errors that broaden via the caller.
pub(crate) fn base_edges(
    root: &Path,
    base: &str,
    head: &str,
    discovery: &Discovery,
) -> Result<Vec<LocalEdge>, String> {
    let mut packages: Vec<(String, String)> = Vec::new();
    let mut wanted: Vec<(String, String)> = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if package.in_workspace && !package.external {
                packages.push((manifest_dir(&package.manifest), package.id.clone()));
                wanted.push((package.manifest.clone(), package.id.clone()));
            }
        }
    }
    let added = added_files(root, base, head)?;
    wanted.retain(|(manifest, _)| !added.contains(manifest));
    if wanted.len() > MAX_BASE_MANIFEST_BATCH {
        return Err("base_batch_over_cap".to_owned());
    }
    let specs: Vec<&str> = wanted
        .iter()
        .map(|(manifest, _)| manifest.as_str())
        .collect();
    let texts = base_manifests(root, base, &specs)?;
    let mut edges = Vec::new();
    for ((manifest, id), text) in wanted.iter().zip(texts.iter()) {
        edges.extend(manifest_edges(
            text,
            id,
            &manifest_dir(manifest),
            &packages,
        )?);
    }
    Ok(edges)
}

/// Base contents of every wanted manifest via one `git show` each.
///
/// One batched `git show` cannot delimit blobs: git shows a repeated
/// separator object only once, so per-manifest reads keep boundaries exact.
/// Added (new) manifests never reach here.
fn base_manifests(root: &Path, base: &str, manifests: &[&str]) -> Result<Vec<String>, String> {
    validate_diff_rev(base, "bad_base")?;
    let mut out = Vec::with_capacity(manifests.len());
    for manifest in manifests {
        let args = vec![OsString::from(format!("{base}:{manifest}"))];
        validate_select_show_args(&args).map_err(|err| err.to_string())?;
        let output = GitRequest::show(args)
            .run_in(root)
            .map_err(|err| err.to_string())?;
        output
            .require_success("git")
            .map_err(|err| err.to_string())?;
        out.push(output.stdout_text("git").map_err(|err| err.to_string())?);
    }
    Ok(out)
}

/// Path edges of one base manifest, resolved to head package IDs.
fn manifest_edges(
    text: &str,
    from: &str,
    dir: &str,
    packages: &[(String, String)],
) -> Result<Vec<LocalEdge>, String> {
    let document: toml::Table = toml::from_str(text).map_err(|err| err.to_string())?;
    let mut edges = Vec::new();
    for (table, kind) in sections() {
        if let Some(deps) = document.get(table).and_then(toml::Value::as_table) {
            edges.extend(dep_edges(deps, from, dir, packages, kind, None));
        }
    }
    if let Some(targets) = document.get("target").and_then(toml::Value::as_table) {
        for (name, target) in targets {
            let Some(target) = target.as_table() else {
                continue;
            };
            for (table, kind) in sections() {
                if let Some(deps) = target.get(table).and_then(toml::Value::as_table) {
                    edges.extend(dep_edges(deps, from, dir, packages, kind, Some(name)));
                }
            }
        }
    }
    Ok(edges)
}

/// Dependency tables with their edge kinds.
fn sections() -> [(&'static str, DepKind); 3] {
    [
        ("dependencies", DepKind::Normal),
        ("build-dependencies", DepKind::Build),
        ("dev-dependencies", DepKind::Dev),
    ]
}

/// Path-dep edges of one dependency table.
fn dep_edges(
    deps: &toml::Table,
    from: &str,
    dir: &str,
    packages: &[(String, String)],
    kind: DepKind,
    target: Option<&str>,
) -> Vec<LocalEdge> {
    let mut edges = Vec::new();
    for spec in deps.values() {
        let Some(spec) = spec.as_table() else {
            continue;
        };
        let Some(path) = spec.get("path").and_then(toml::Value::as_str) else {
            continue;
        };
        let joined = join_dir(dir, path);
        let Some(to) = packages
            .iter()
            .find(|(owned, _)| *owned == joined)
            .map(|(_, id)| id)
        else {
            continue;
        };
        let optional = spec
            .get("optional")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        edges.push(LocalEdge {
            from: from.to_owned(),
            to: to.clone(),
            kind,
            optional,
            target: target.map(str::to_owned),
        });
    }
    edges
}

/// Join a manifest directory with a dep path, resolving `.` and `..`.
fn join_dir(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|seg| !seg.is_empty()).collect();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    parts.join("/")
}
