// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use toml::Value;

use crate::context::FreshnessContext;

use super::super::{CRATES_IO, DeclaredDependency, MemberOwners, MemberWorkspace, path_text};

use super::{source, version};

pub(super) fn check_declared(
    ctx: &mut FreshnessContext,
    relative: &str,
    declared: &[DeclaredDependency],
    by_name: &BTreeMap<String, Vec<Value>>,
    members: &BTreeSet<String>,
    owners: &MemberOwners,
) -> BTreeSet<String> {
    let mut passed = 0;
    let mut failed = 0;
    let mut path_packages = BTreeSet::new();
    for dependency in declared {
        if check_requirement(
            ctx,
            dependency,
            by_name,
            members,
            owners,
            &mut path_packages,
        ) {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    let subject = if relative.is_empty() {
        "(declared-summary)".to_owned()
    } else {
        format!("{relative}/(declared-summary)")
    };
    ctx.info_row(
        "lock-staleness",
        &subject,
        &format!(
            "{passed} match, {failed} fail, {} locked names retained",
            by_name.len()
        ),
    );
    path_packages
}

fn check_requirement(
    ctx: &mut FreshnessContext,
    dependency: &DeclaredDependency,
    by_name: &BTreeMap<String, Vec<Value>>,
    members: &BTreeSet<String>,
    owners: &MemberOwners,
    path_packages: &mut BTreeSet<String>,
) -> bool {
    let Some(want) = dependency.requirement.strip_prefix('=') else {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "requirement {:?} is not exact `=x.y.z` (VER-2.26)",
                dependency.requirement
            ),
        );
        return false;
    };
    let want = want.split('+').next().unwrap_or("");
    if dependency.path.is_some()
        && validate_path_dependency(ctx, dependency, owners, want).is_none()
    {
        return false;
    }
    let matches = by_name
        .get(&dependency.name)
        .into_iter()
        .flatten()
        .filter(|entry| version(entry) == Some(want))
        .cloned()
        .collect::<Vec<_>>();
    if matches.is_empty() {
        let locked = by_name
            .get(&dependency.name)
            .into_iter()
            .flatten()
            .filter_map(|entry| version(entry))
            .collect::<BTreeSet<_>>();
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "declared {:?} has no locked identity (locked versions: {locked:?})",
                dependency.requirement
            ),
        );
        return false;
    }
    if check_resolved_source(ctx, dependency, &matches, members) {
        if dependency.path.is_some() {
            path_packages.insert(dependency.name.clone());
        }
        true
    } else {
        false
    }
}

fn check_resolved_source(
    ctx: &mut FreshnessContext,
    dependency: &DeclaredDependency,
    matches: &[Value],
    members: &BTreeSet<String>,
) -> bool {
    let sources = matches.iter().map(source).collect::<BTreeSet<_>>();
    if sources.len() != 1 {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "ambiguous identity: {} {} resolves from {sources:?}",
                dependency.name,
                dependency.requirement.trim_start_matches('=')
            ),
        );
        return false;
    }
    let resolved_source = sources.iter().next().map_or("local", String::as_str);
    if dependency.path.is_some() {
        if resolved_source != "local" {
            ctx.fail_row(
                "lock-staleness",
                &dependency.subject,
                &format!("path dependency locked from non-local source {resolved_source}"),
            );
            return false;
        }
        ctx.pass_row(
            "lock-staleness",
            &dependency.subject,
            &format!("{} @ path", dependency.requirement),
        );
        return true;
    }
    if members.contains(&dependency.name) && resolved_source != "local" {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "workspace member {} locked from {resolved_source}",
                dependency.name
            ),
        );
        return false;
    }
    if members.contains(&dependency.name) {
        ctx.pass_row(
            "lock-staleness",
            &dependency.subject,
            &format!("{} @ workspace", dependency.requirement),
        );
        true
    } else if resolved_source != CRATES_IO {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!("locked from non-registry source {resolved_source}"),
        );
        false
    } else {
        ctx.pass_row(
            "lock-staleness",
            &dependency.subject,
            &format!("{} @ registry", dependency.requirement),
        );
        true
    }
}

fn validate_path_dependency(
    ctx: &mut FreshnessContext,
    dependency: &DeclaredDependency,
    owners: &MemberOwners,
    expected_version: &str,
) -> Option<String> {
    let Some(path) = &dependency.path else {
        return None;
    };
    let target = path.base.join(&path.path).join("Cargo.toml");
    let root = match ctx.root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            ctx.fail_row(
                "lock-staleness",
                &dependency.subject,
                &format!("repository root cannot be canonicalized ({error})"),
            );
            return None;
        }
    };
    let canonical = match target.canonicalize() {
        Ok(canonical) => canonical,
        Err(error) => {
            ctx.fail_row(
                "lock-staleness",
                &dependency.subject,
                &format!("path target {} is unreadable ({error})", path_text(&target)),
            );
            return None;
        }
    };
    if !canonical.starts_with(&root) {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "path target {} escapes the repository root",
                path_text(&canonical)
            ),
        );
        return None;
    }
    let Some(candidates) = owners.get(&canonical) else {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "path target {} is not a discovered workspace package",
                path_text(&canonical)
            ),
        );
        return None;
    };
    if candidates.len() != 1 {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "path target {} belongs to {} discovered workspaces, expected exactly one",
                path_text(&canonical),
                candidates.len()
            ),
        );
        return None;
    }
    validate_path_manifest(
        ctx,
        dependency,
        &canonical,
        &candidates[0],
        expected_version,
    )
}

fn validate_path_manifest(
    ctx: &mut FreshnessContext,
    dependency: &DeclaredDependency,
    manifest: &Path,
    owner: &MemberWorkspace,
    expected_version: &str,
) -> Option<String> {
    let text = match fs::read_to_string(manifest) {
        Ok(text) => text,
        Err(error) => {
            ctx.fail_row(
                "lock-staleness",
                &dependency.subject,
                &format!(
                    "path target {} is unreadable ({error})",
                    path_text(manifest)
                ),
            );
            return None;
        }
    };
    let document: Value = match toml::from_str(&text) {
        Ok(document) => document,
        Err(error) => {
            ctx.fail_row(
                "lock-staleness",
                &dependency.subject,
                &format!(
                    "path target {} is invalid TOML ({error})",
                    path_text(manifest)
                ),
            );
            return None;
        }
    };
    validate_package_identity(
        ctx,
        dependency,
        &document,
        owner,
        expected_version,
        manifest,
    )
}

fn validate_package_identity(
    ctx: &mut FreshnessContext,
    dependency: &DeclaredDependency,
    document: &Value,
    owner: &MemberWorkspace,
    expected_version: &str,
    manifest: &Path,
) -> Option<String> {
    let Some(package) = document.get("package").and_then(Value::as_table) else {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!("path target {} has no package table", path_text(manifest)),
        );
        return None;
    };
    let Some(name) = package.get("name").and_then(Value::as_str) else {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!("path target {} has no package name", path_text(manifest)),
        );
        return None;
    };
    if name != dependency.name {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "path target package name {name:?} != declared package {:?}",
                dependency.name
            ),
        );
        return None;
    }
    let Some(version) = effective_package_version(package, &owner.document) else {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "path target package {name} has no effective version from workspace {}",
                path_text(&owner.root)
            ),
        );
        return None;
    };
    if version.split('+').next() != Some(expected_version) {
        ctx.fail_row(
            "lock-staleness",
            &dependency.subject,
            &format!(
                "path target package {name} version {version:?} != exact requirement ={expected_version}"
            ),
        );
        return None;
    }
    Some(name.to_owned())
}

fn effective_package_version(
    package: &toml::map::Map<String, Value>,
    owner: &Value,
) -> Option<String> {
    if let Some(version) = package.get("version").and_then(Value::as_str) {
        return Some(version.to_owned());
    }
    if package
        .get("version")
        .and_then(Value::as_table)
        .and_then(|version| version.get("workspace"))
        .and_then(Value::as_bool)
        != Some(true)
    {
        return None;
    }
    owner
        .get("workspace")
        .and_then(Value::as_table)
        .and_then(|workspace| workspace.get("package"))
        .and_then(Value::as_table)
        .and_then(|package| package.get("version"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}
