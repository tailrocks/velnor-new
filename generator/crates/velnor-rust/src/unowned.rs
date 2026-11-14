// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use toml::{Table, Value};
use velnor_model::{
    DependencyKind, MemberDependency, PackageName, RepoPath, RepoView, RustRoot, UnownedPackage,
    Unread,
};

use crate::roots::{Targets, within};
use crate::toml_file::table;

const SECTIONS: [(&str, DependencyKind); 3] = [
    ("dependencies", DependencyKind::Normal),
    ("build-dependencies", DependencyKind::Build),
    ("build_dependencies", DependencyKind::Build),
];

enum Source {
    Path(RepoPath, PackageName),
    Named(PackageName),
    Inherited,
}

type Section<'t> = Result<(String, &'t Table, DependencyKind), Unread>;

pub(crate) fn unowned(
    view: &RepoView,
    roots: &[RustRoot],
    targets: &Targets,
) -> BTreeMap<RepoPath, UnownedPackage> {
    let members: BTreeSet<&RepoPath> = roots
        .iter()
        .flat_map(|root| &root.members)
        .map(|member| &member.manifest)
        .collect();
    let mut pending: Vec<MemberDependency> = roots
        .iter()
        .flat_map(|root| &root.members)
        .flat_map(|member| member.dependencies.iter().cloned())
        .collect();
    let mut found = BTreeMap::new();
    while let Some(dependency) = pending.pop() {
        if members.contains(&dependency.manifest) || found.contains_key(&dependency.manifest) {
            continue;
        }
        let package = package(view, targets, &dependency.manifest, dependency.name);
        pending.extend(package.dependencies.iter().cloned());
        found.insert(dependency.manifest, package);
    }
    found
}

fn package(
    view: &RepoView,
    targets: &Targets,
    manifest: &RepoPath,
    name: PackageName,
) -> UnownedPackage {
    let parsed = view
        .files
        .get(manifest)
        .ok_or(Unread::Missing)
        .and_then(|bytes| table(bytes).map_err(|reason| Unread::Invalid(reason.to_string())));
    let own = match parsed {
        Ok(own) => own,
        Err(unread) => {
            return UnownedPackage {
                name,
                dependencies: Vec::new(),
                unread: Some(unread),
            };
        }
    };
    let mut package = UnownedPackage {
        name: own
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(Value::as_str)
            .and_then(|declared| PackageName::new(declared).ok())
            .unwrap_or(name),
        dependencies: Vec::new(),
        unread: None,
    };
    for section in sections(&own) {
        let (scope, entries, kind) = match section {
            Ok(section) => section,
            Err(unread) => {
                package.unread.get_or_insert(unread);
                continue;
            }
        };
        for (key, entry) in entries {
            match edges(
                view,
                targets,
                manifest,
                &own,
                key,
                &format!("{scope}.{key}"),
                entry,
            ) {
                Ok(edges) => {
                    package
                        .dependencies
                        .extend(edges.into_iter().map(|(manifest, name)| MemberDependency {
                            name,
                            kind,
                            manifest,
                        }));
                }
                Err(unread) => {
                    package.unread.get_or_insert(unread);
                }
            }
        }
    }
    package
}

fn sections(own: &Table) -> Vec<Section<'_>> {
    let mut scopes = vec![Ok((String::new(), own))];
    match own.get("target") {
        None => {}
        Some(Value::Table(platforms)) => {
            scopes.extend(platforms.iter().map(|(platform, scope)| {
                scope
                    .as_table()
                    .map(|scope| (format!("target.{platform}."), scope))
                    .ok_or_else(|| Unread::Invalid(format!("target.{platform}")))
            }));
        }
        Some(_) => scopes.push(Err(Unread::Invalid("target".to_owned()))),
    }
    scopes
        .into_iter()
        .flat_map(|scope| match scope {
            Err(unread) => vec![Err(unread)],
            Ok((prefix, table)) => SECTIONS
                .iter()
                .filter_map(|(name, kind)| {
                    let at = format!("{prefix}{name}");
                    let entries = table.get(*name)?;
                    Some(
                        entries
                            .as_table()
                            .map(|entries| (at.clone(), entries, *kind))
                            .ok_or(Unread::Invalid(at)),
                    )
                })
                .collect(),
        })
        .collect()
}

fn edges(
    view: &RepoView,
    targets: &Targets,
    manifest: &RepoPath,
    own: &Table,
    key: &str,
    at: &str,
    entry: &Value,
) -> Result<Vec<(RepoPath, PackageName)>, Unread> {
    let source = match source(manifest.parent().as_ref(), key, at, entry)? {
        Source::Inherited => inherited(view, manifest, own, key, at)?,
        resolved => resolved,
    };
    Ok(match source {
        Source::Path(target, name) => vec![(target, name)],
        Source::Named(name) => targets
            .get(&name)
            .into_iter()
            .flatten()
            .map(|target| (target.clone(), name.clone()))
            .collect(),
        Source::Inherited => Vec::new(),
    })
}

fn source(
    directory: Option<&RepoPath>,
    key: &str,
    at: &str,
    entry: &Value,
) -> Result<Source, Unread> {
    let invalid = || Unread::Invalid(at.to_owned());
    let fields = match entry {
        Value::String(_) => {
            return PackageName::new(key)
                .map(Source::Named)
                .map_err(|_| invalid());
        }
        Value::Table(fields) => fields,
        _ => return Err(invalid()),
    };
    if fields.contains_key("workspace") {
        return Ok(Source::Inherited);
    }
    if fields.contains_key("git") {
        return Err(Unread::Git(key.to_owned()));
    }
    let name = match fields.get("package") {
        None => key,
        Some(Value::String(package)) => package,
        Some(_) => return Err(invalid()),
    };
    let name = PackageName::new(name).map_err(|_| invalid())?;
    match fields.get("path") {
        None => Ok(Source::Named(name)),
        Some(Value::String(path)) => within(directory, path)
            .filter(|_| !path.starts_with('/'))
            .ok_or_else(|| Unread::Outside(key.to_owned()))
            .and_then(|target| RepoPath::new(target).map_err(|_| invalid()))
            .map(|target| Source::Path(target, name)),
        Some(_) => Err(invalid()),
    }
}

fn inherited(
    view: &RepoView,
    manifest: &RepoPath,
    own: &Table,
    key: &str,
    at: &str,
) -> Result<Source, Unread> {
    let unresolved = || Unread::Inherited(key.to_owned());
    let (root, workspace) = workspace(view, manifest, own).ok_or_else(unresolved)?;
    let entry = workspace
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(|dependencies| dependencies.get(key))
        .ok_or_else(unresolved)?;
    match source(root.parent().as_ref(), key, at, entry)? {
        Source::Inherited => Err(unresolved()),
        resolved => Ok(resolved),
    }
}

fn workspace(view: &RepoView, manifest: &RepoPath, own: &Table) -> Option<(RepoPath, Table)> {
    let directory = manifest.parent();
    if let Some(declared) = own
        .get("package")
        .and_then(|package| package.get("workspace"))
    {
        let root = RepoPath::new(within(directory.as_ref(), declared.as_str()?)?).ok()?;
        let parsed = table(view.files.get(&root)?).ok()?;
        return parsed.contains_key("workspace").then_some((root, parsed));
    }
    if own.contains_key("workspace") {
        return Some((manifest.clone(), own.clone()));
    }
    let mut folder = directory;
    while let Some(current) = folder {
        folder = current.parent();
        let candidate = RepoPath::new(within(folder.as_ref(), ".")?).ok()?;
        match view.files.get(&candidate) {
            Some(bytes) => {
                let parsed = table(bytes).ok()?;
                if parsed.contains_key("workspace") {
                    return Some((candidate, parsed));
                }
            }
            None if view.tracked_paths.contains(&candidate) => return None,
            None => {}
        }
    }
    None
}
