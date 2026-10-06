//! Cargo workspace discovery and declared dependency parsing.

use std::collections::{BTreeSet, VecDeque};

use std::path::{Path, PathBuf};

use toml::Value;

use crate::context::FreshnessContext;

use crate::patterns;

use super::{
    DEP_SECTIONS, DeclaredDependency, load_manifest, manifest_label, path_text, read_toml_row,
};

pub(super) fn discover_workspaces(ctx: &mut FreshnessContext) -> Vec<(String, Value)> {
    let mut pending = VecDeque::from([(String::new(), load_manifest(ctx, "Cargo.toml"))]);
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    while let Some((relative, document)) = pending.pop_front() {
        if !seen.insert(relative.clone()) {
            continue;
        }
        let Some(document) = document else { continue };
        let Some(workspace) = document.get("workspace").and_then(Value::as_table) else {
            ctx.fail_row(
                "lock-staleness",
                &manifest_label(&relative),
                "no [workspace] table",
            );
            continue;
        };
        roots.push((relative.clone(), document.clone()));
        let Some(exclude_entries) = workspace.get("exclude") else {
            continue;
        };
        let Some(exclude_entries) = exclude_entries.as_array() else {
            ctx.fail_row(
                "lock-staleness",
                &manifest_label(&relative),
                "workspace.exclude must be an array",
            );
            continue;
        };
        for excluded_entry in exclude_entries {
            let Some(excluded_path) = excluded_entry.as_str() else {
                ctx.fail_row(
                    "lock-staleness",
                    &manifest_label(&relative),
                    &format!("workspace.exclude entry is not a string: {excluded_entry}"),
                );
                continue;
            };
            let path = ctx.root.join(&relative).join(excluded_path);
            let hits = match patterns::expand(
                &ctx.root,
                &path
                    .strip_prefix(&ctx.root)
                    .unwrap_or(&path)
                    .to_string_lossy(),
            ) {
                Ok(hits) => hits,
                Err(error) => {
                    ctx.fail_row("lock-staleness", &manifest_label(&relative), &error);
                    continue;
                }
            };
            for hit in hits {
                let manifest = if hit.file_name().is_some_and(|name| name == "Cargo.toml") {
                    hit
                } else {
                    hit.join("Cargo.toml")
                };
                let child = manifest
                    .strip_prefix(&ctx.root)
                    .map_or_else(|_| manifest.clone(), Path::to_path_buf);
                if let Some(child_relative) = child.parent() {
                    let key = path_text(child_relative);
                    if let Some(nested) = load_manifest(ctx, &path_text(&child))
                        && nested.get("workspace").is_some_and(Value::is_table)
                    {
                        pending.push_back((key, Some(nested)));
                    }
                }
            }
        }
    }
    roots
}
pub(super) fn member_manifests(
    ctx: &mut FreshnessContext,
    relative: &str,
    document: &Value,
) -> Vec<PathBuf> {
    let workspace_dir = ctx.root.join(relative);
    let mut manifests = Vec::new();
    if document.get("package").is_some_and(Value::is_table) {
        manifests.push(workspace_dir.join("Cargo.toml"));
    }
    let members = document
        .get("workspace")
        .and_then(Value::as_table)
        .and_then(|table| table.get("members"));
    let Some(members) = members.and_then(Value::as_array) else {
        if members.is_some() {
            ctx.fail_row(
                "lock-staleness",
                &manifest_label(relative),
                "workspace.members must be an array",
            );
        }
        return manifests;
    };
    for member in members {
        let Some(member) = member.as_str() else {
            ctx.fail_row(
                "lock-staleness",
                &manifest_label(relative),
                &format!("workspace member is not a string: {member}"),
            );
            continue;
        };
        let base = workspace_dir.join(member);
        let pattern = base.join("Cargo.toml");
        let pattern = path_text(pattern.strip_prefix(&ctx.root).unwrap_or(&pattern));
        let hits = match patterns::expand(&ctx.root, &pattern) {
            Ok(hits) => hits,
            Err(error) => {
                ctx.fail_row("lock-staleness", &format!("{relative}/{member}"), &error);
                continue;
            }
        };
        if hits.is_empty() {
            ctx.fail_row(
                "lock-staleness",
                &format!("{relative}/{member}"),
                "workspace member manifest not found",
            );
        }
        manifests.extend(hits);
    }
    manifests.sort();
    manifests.dedup();
    manifests
}

pub(super) fn read_member_manifests(
    ctx: &mut FreshnessContext,
    manifests: &[PathBuf],
    inherited: Option<&Value>,
) -> (BTreeSet<String>, Vec<DeclaredDependency>) {
    let mut members = BTreeSet::new();
    let mut declared = Vec::new();
    for manifest in manifests {
        let Some(document) = read_toml_row(ctx, manifest, "lock-staleness") else {
            continue;
        };
        let Some(package) = document.get("package").and_then(Value::as_table) else {
            ctx.fail_row(
                "lock-staleness",
                &path_text(manifest),
                "workspace member has no package name",
            );
            continue;
        };
        let Some(crate_name) = package.get("name").and_then(Value::as_str) else {
            ctx.fail_row(
                "lock-staleness",
                &path_text(manifest),
                "workspace member has no package name",
            );
            continue;
        };
        members.insert(crate_name.to_owned());
        walk_dependency_tables(ctx, &document, crate_name, inherited, "", &mut declared);
    }
    (members, declared)
}

fn walk_dependency_tables(
    ctx: &mut FreshnessContext,
    node: &Value,
    crate_name: &str,
    inherited: Option<&Value>,
    prefix: &str,
    output: &mut Vec<DeclaredDependency>,
) {
    let Some(table) = node.as_table() else { return };
    for section in DEP_SECTIONS {
        let Some(dependencies) = table.get(*section) else {
            continue;
        };
        let Some(dependencies) = dependencies.as_table() else {
            ctx.fail_row(
                "lock-staleness",
                &format!("{crate_name}:{prefix}{section}"),
                "dependency section must be a table",
            );
            continue;
        };
        for (alias, spec) in dependencies {
            if let Some(dependency) = declared_dependency(
                ctx,
                crate_name,
                &format!("{prefix}{section}"),
                alias,
                spec,
                inherited,
            ) {
                output.push(dependency);
            }
        }
    }
    let Some(targets) = table.get("target").and_then(Value::as_table) else {
        return;
    };
    for (target, body) in targets {
        walk_dependency_tables(
            ctx,
            body,
            crate_name,
            inherited,
            &format!("target.{target}."),
            output,
        );
    }
}

fn declared_dependency(
    ctx: &mut FreshnessContext,
    crate_name: &str,
    scope: &str,
    alias: &str,
    spec: &Value,
    inherited: Option<&Value>,
) -> Option<DeclaredDependency> {
    let subject = format!("{crate_name}:{scope}:{alias}");
    if let Some(requirement) = spec.as_str() {
        return Some(DeclaredDependency {
            subject,
            name: alias.to_owned(),
            requirement: requirement.to_owned(),
        });
    }
    let Some(spec) = spec.as_table() else {
        ctx.fail_row(
            "lock-staleness",
            &subject,
            &format!("malformed spec {spec}"),
        );
        return None;
    };
    if spec
        .get("git")
        .and_then(Value::as_str)
        .is_some_and(|git| !git.is_empty())
    {
        ctx.fail_row(
            "lock-staleness",
            &subject,
            &format!("git dependency forbidden ({:?})", spec.get("git")),
        );
        return None;
    }
    let real = spec.get("package").and_then(Value::as_str).unwrap_or(alias);
    if let Some(requirement) = spec.get("version").and_then(Value::as_str) {
        return Some(DeclaredDependency {
            subject,
            name: real.to_owned(),
            requirement: requirement.to_owned(),
        });
    }
    if spec.get("workspace").and_then(Value::as_bool) == Some(true) {
        return inherited_dependency(ctx, &subject, alias, real, inherited);
    }
    if spec.contains_key("path") {
        ctx.pass_row(
            "lock-staleness",
            &subject,
            "path-only, no registry identity",
        );
        return None;
    }
    ctx.fail_row(
        "lock-staleness",
        &subject,
        &format!("no version, workspace, or path in {spec:?}"),
    );
    None
}

fn inherited_dependency(
    ctx: &mut FreshnessContext,
    subject: &str,
    alias: &str,
    real: &str,
    inherited: Option<&Value>,
) -> Option<DeclaredDependency> {
    let dependencies = inherited.and_then(Value::as_table);
    let spec = dependencies.and_then(|table| table.get(real).or_else(|| table.get(alias)));
    let real = spec
        .and_then(Value::as_table)
        .and_then(|table| table.get("package"))
        .and_then(Value::as_str)
        .unwrap_or(real);
    let requirement = spec.and_then(Value::as_str).or_else(|| {
        spec.and_then(Value::as_table)
            .and_then(|table| table.get("version"))
            .and_then(Value::as_str)
    });
    if let Some(requirement) = requirement {
        Some(DeclaredDependency {
            subject: subject.to_owned(),
            name: real.to_owned(),
            requirement: requirement.to_owned(),
        })
    } else {
        ctx.fail_row(
            "lock-staleness",
            subject,
            &format!("workspace inheritance unresolvable: [workspace.dependencies] lacks {real:?}"),
        );
        None
    }
}
