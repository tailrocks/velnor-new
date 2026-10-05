//! Discover Cargo workspaces and check each independent lock and policy.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use super::context::FreshnessContext;
use super::dependencies;
use super::dependency_graph::{self, Declaration};
use super::glob;
use super::workspace_discovery;
use super::workspace_paths::{manifest_subject, path_text, toml_repr};

const CHECK: &str = "lock-staleness";

pub(super) fn check_all(ctx: &mut FreshnessContext) {
    let workspaces = workspace_discovery::discover(ctx);
    ctx.workspace_roots = workspaces
        .iter()
        .map(|(relative, _)| relative.clone())
        .collect();
    ctx.locked = Some(Vec::new());
    ctx.member_names.clear();
    for (relative, document) in workspaces {
        if let Some(locked) = check_workspace(ctx, &relative, &document)
            && let Some(all_locked) = &mut ctx.locked
        {
            all_locked.extend(locked);
        }
    }
}

pub(super) fn check_advisory_policies(ctx: &mut FreshnessContext) {
    if ctx.workspace_roots.is_empty() {
        ctx.fail_row(
            "advisories",
            "(workspaces)",
            "no Cargo workspaces discovered",
        );
        return;
    }
    let roots = ctx.workspace_roots.clone();
    for relative in roots {
        super::advisories::check_deny_policy_at(ctx, &relative.join("deny.toml"));
    }
    if ctx.with_advisories {
        for relative in ctx.workspace_roots.clone() {
            super::advisories::check_live_advisories(ctx, &relative);
        }
    } else {
        ctx.info_row(
            "advisories",
            "live scan",
            "runs as the CI Cargo Deny job; --with-advisories runs it here",
        );
    }
}

fn check_workspace(
    ctx: &mut FreshnessContext,
    relative: &Path,
    document: &TomlValue,
) -> Option<Vec<JsonValue>> {
    let workspace = document.get("workspace").and_then(TomlValue::as_table)?;
    let inherited = match workspace.get("dependencies") {
        None => TomlValue::Table(toml::map::Map::new()),
        Some(TomlValue::Table(table)) => TomlValue::Table(table.clone()),
        Some(_) => {
            ctx.fail_row(
                CHECK,
                &manifest_subject(relative),
                "workspace.dependencies must be a table",
            );
            return None;
        }
    };
    let locked = read_lock(ctx, relative)?;
    let manifests = member_manifests(ctx, relative, document);
    if manifests.is_empty() {
        ctx.fail_row(
            CHECK,
            &manifest_subject(relative),
            "no workspace member manifests found",
        );
    }
    let (members, declared) = read_manifests(ctx, &manifests, &inherited);
    ctx.member_names.extend(members.iter().cloned());
    let prefix = if relative.as_os_str().is_empty() {
        String::new()
    } else {
        format!("{}/", path_text(relative))
    };
    dependency_graph::check_scoped(ctx, &locked, &declared, &members, &prefix);
    check_lock_mtime(ctx, relative, &manifests);
    Some(locked)
}

fn member_manifests(
    ctx: &mut FreshnessContext,
    relative: &Path,
    document: &TomlValue,
) -> Vec<PathBuf> {
    let directory = ctx.root.join(relative);
    let mut manifests = Vec::new();
    if document
        .get("package")
        .and_then(TomlValue::as_table)
        .is_some()
    {
        manifests.push(directory.join("Cargo.toml"));
    }
    let Some(workspace) = document.get("workspace").and_then(TomlValue::as_table) else {
        return manifests;
    };
    let empty = Vec::new();
    let members = match workspace.get("members") {
        None => &empty,
        Some(TomlValue::Array(members)) => members,
        Some(_) => {
            ctx.fail_row(
                CHECK,
                &manifest_subject(relative),
                "workspace.members must be an array",
            );
            return manifests;
        }
    };
    for member in members {
        let Some(member) = member.as_str() else {
            ctx.fail_row(
                CHECK,
                &manifest_subject(relative),
                &format!("workspace member is not a string: {}", toml_repr(member)),
            );
            continue;
        };
        let member_path = relative.join(member);
        let pattern = member_path.join("Cargo.toml");
        let matches = match glob::matching_entries(&ctx.root, &path_text(&pattern)) {
            Ok(matches) => matches,
            Err(error) => {
                ctx.fail_row(
                    CHECK,
                    &manifest_subject(relative),
                    &format!("invalid workspace member glob ({error})"),
                );
                continue;
            }
        };
        let member_manifests = matches
            .into_iter()
            .filter(|entry| entry.is_file)
            .map(|entry| ctx.root.join(entry.path))
            .collect::<Vec<_>>();
        if member_manifests.is_empty() {
            ctx.fail_row(
                CHECK,
                &path_text(&relative.join(member)),
                "workspace member manifest not found",
            );
        }
        manifests.extend(member_manifests);
    }
    manifests.sort();
    manifests.dedup();
    manifests
}

fn read_manifests(
    ctx: &mut FreshnessContext,
    manifests: &[PathBuf],
    inherited: &TomlValue,
) -> (BTreeSet<String>, Vec<Declaration>) {
    let mut members = BTreeSet::new();
    let mut declared = Vec::new();
    for manifest in manifests {
        let Some(document) = dependencies::read_manifest(ctx, manifest) else {
            continue;
        };
        let package_name = document
            .get("package")
            .and_then(TomlValue::as_table)
            .and_then(|package| package.get("name"))
            .and_then(TomlValue::as_str)
            .filter(|name| !name.is_empty());
        let Some(package_name) = package_name else {
            ctx.fail_row(
                CHECK,
                &path_text(manifest.strip_prefix(&ctx.root).unwrap_or(manifest)),
                "workspace member has no package name",
            );
            continue;
        };
        members.insert(package_name.to_owned());
        for (scope, alias, spec) in dependencies::walk_dep_tables(&document) {
            if let Some(declaration) = dependencies::declared_dependency(
                ctx,
                package_name,
                &scope,
                &alias,
                &spec,
                inherited,
            ) {
                declared.push(declaration);
            }
        }
    }
    (members, declared)
}

fn read_lock(ctx: &mut FreshnessContext, relative: &Path) -> Option<Vec<JsonValue>> {
    let lock_path = relative.join("Cargo.lock");
    let subject = path_text(&lock_path);
    let text = match fs::read_to_string(ctx.root.join(&lock_path)) {
        Ok(text) => text,
        Err(error) => {
            ctx.fail_row(CHECK, &subject, &format!("unreadable ({error})"));
            return None;
        }
    };
    let document = match toml::from_str::<TomlValue>(&text) {
        Ok(document) => document,
        Err(error) => {
            ctx.fail_row(CHECK, &subject, &format!("unreadable ({error})"));
            return None;
        }
    };
    let packages = match document.get("package") {
        None => Vec::new(),
        Some(TomlValue::Array(packages)) => packages.clone(),
        Some(value) => {
            ctx.fail_row(
                CHECK,
                &subject,
                &format!("package field must be an array, got {}", toml_repr(value)),
            );
            return None;
        }
    };
    packages
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_or_else(
            |error| {
                ctx.fail_row(CHECK, &subject, &format!("unreadable ({error})"));
                None
            },
            Some,
        )
}

fn check_lock_mtime(ctx: &mut FreshnessContext, relative: &Path, manifests: &[PathBuf]) {
    let lock = ctx.root.join(relative).join("Cargo.lock");
    let subject = path_text(&relative.join("Cargo.lock"));
    let result = fs::metadata(lock)
        .and_then(|metadata| metadata.modified())
        .and_then(|lock_time| {
            manifests
                .iter()
                .map(|path| fs::metadata(path).and_then(|metadata| metadata.modified()))
                .collect::<Result<Vec<_>, _>>()
                .map(|times| (lock_time, times.into_iter().max()))
        });
    match result {
        Ok((lock_time, Some(newest))) => ctx.info_row(
            "lock-mtime",
            &subject,
            &format!("lock_is_newest={}", lock_time >= newest),
        ),
        Ok((_, None)) => ctx.info_row("lock-mtime", &subject, "mtime unreadable (no manifests)"),
        Err(error) => ctx.info_row(
            "lock-mtime",
            &subject,
            &format!("mtime unreadable ({error})"),
        ),
    }
}

#[cfg(test)]
#[path = "nested_workspaces_tests.rs"]
mod tests;
