// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ops::Range;

use toml::Spanned;
use toml::de::{DeTable, DeValue};

use crate::error::CiError;

#[derive(Debug, Clone)]
pub struct Source {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Root {
    pub manifests: Vec<Source>,
    pub lock: Option<Source>,
}

#[derive(Debug)]
pub struct Aligned {
    pub edits: Vec<Source>,
    pub report: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);

impl Version {
    fn parse(what: &str, text: &str) -> Result<Self, CiError> {
        let mut parts = text.split('.').map(|part| {
            let plain = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
            plain.then(|| part.parse::<u64>().ok()).flatten()
        });
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) => {
                Ok(Self(major, minor, patch))
            }
            _ => Err(CiError::new(format!(
                "{what} version `{}` is not a plain MAJOR.MINOR.PATCH",
                text.escape_debug()
            ))),
        }
    }
}

struct Parsed<'a> {
    path: &'a str,
    doc: Spanned<DeTable<'a>>,
}

impl<'a> Parsed<'a> {
    fn new(source: &'a Source) -> Result<Self, CiError> {
        let doc = DeTable::parse(&source.text)
            .map_err(|e| CiError::new(format!("{} is not valid TOML: {e}", source.path)))?;
        Ok(Self {
            path: &source.path,
            doc,
        })
    }

    fn package(&self) -> Option<&DeTable<'a>> {
        table(self.doc.get_ref().get("package")?)
    }

    fn name(&self) -> Option<&str> {
        self.package()?.get("name")?.get_ref().as_str()
    }
}

struct Edit {
    path: String,
    span: Range<usize>,
    text: String,
}

fn table<'a, 'i>(value: &'a Spanned<DeValue<'i>>) -> Option<&'a DeTable<'i>> {
    value.get_ref().as_table()
}

fn quoted(version: &str) -> String {
    ["\"", version, "\""].concat()
}

pub fn workspace_members(path: &str, text: &str) -> Result<Vec<String>, CiError> {
    let doc =
        DeTable::parse(text).map_err(|e| CiError::new(format!("{path} is not valid TOML: {e}")))?;
    let Some(workspace) = doc.get_ref().get("workspace").and_then(table) else {
        return Ok(Vec::new());
    };
    let list = |key: &str| -> Result<Vec<String>, CiError> {
        let Some(value) = workspace.get(key) else {
            return Ok(Vec::new());
        };
        let items = value
            .get_ref()
            .as_array()
            .ok_or_else(|| CiError::new(format!("{path}: workspace.{key} is not an array")))?;
        items
            .iter()
            .map(|item| {
                item.get_ref().as_str().map(str::to_owned).ok_or_else(|| {
                    CiError::new(format!("{path}: workspace.{key} holds a non-string"))
                })
            })
            .collect()
    };
    let excluded = list("exclude")?;
    Ok(list("members")?
        .into_iter()
        .filter(|member| !excluded.contains(member))
        .collect())
}

fn literal_version<'a>(
    manifest: &'a Parsed<'_>,
    what: &str,
) -> Result<(&'a str, Range<usize>), CiError> {
    let value = manifest
        .package()
        .and_then(|package| package.get("version"))
        .ok_or_else(|| CiError::new(format!("{}: {what} has no package.version", manifest.path)))?;
    value
        .get_ref()
        .as_str()
        .map(|text| (text, value.span()))
        .ok_or_else(|| {
            CiError::new(format!(
                "{}: the version of {what} is not a literal string (it is inherited from the workspace)",
                manifest.path
            ))
        })
}

fn find<'a, 'b>(
    manifests: &'b [Parsed<'a>],
    name: &str,
    side: &str,
) -> Result<&'b Parsed<'a>, CiError> {
    let mut found = manifests
        .iter()
        .filter(|manifest| manifest.name() == Some(name));
    match (found.next(), found.next()) {
        (Some(manifest), None) => Ok(manifest),
        (None, _) => Err(CiError::new(format!(
            "package `{name}` is not in the {side} workspace, so it is not a member of the release group"
        ))),
        (Some(_), Some(_)) => Err(CiError::new(format!(
            "package `{name}` appears twice in the {side} workspace"
        ))),
    }
}

fn dependency_tables<'a, 'i>(doc: &'a DeTable<'i>) -> Vec<&'a DeTable<'i>> {
    let mut found = Vec::new();
    let mut gather = |parent: &'a DeTable<'i>| {
        for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
            found.extend(parent.get(key).and_then(table));
        }
    };
    gather(doc);
    if let Some(targets) = doc.get("target").and_then(table) {
        for (_, target) in targets {
            if let Some(target) = table(target) {
                gather(target);
            }
        }
    }
    if let Some(workspace) = doc.get("workspace").and_then(table) {
        gather(workspace);
    }
    found
}

fn requirement_edits(
    manifest: &Parsed<'_>,
    current: &BTreeMap<&str, Version>,
    target_text: &str,
    edits: &mut Vec<Edit>,
) -> Result<(), CiError> {
    for dependencies in dependency_tables(manifest.doc.get_ref()) {
        for (key, value) in dependencies {
            let entry = table(value);
            let name = entry
                .and_then(|entry| entry.get("package"))
                .and_then(|package| package.get_ref().as_str())
                .unwrap_or_else(|| key.get_ref());
            let Some(&old) = current.get(name) else {
                continue;
            };
            let requirement = match entry {
                Some(entry) => entry.get("version"),
                None => Some(value),
            };
            let Some((text, span)) =
                requirement.and_then(|req| req.get_ref().as_str().map(|text| (text, req.span())))
            else {
                continue;
            };
            let old_text = format!("={}.{}.{}", old.0, old.1, old.2);
            let new_text = format!("={target_text}");
            if text == new_text {
                continue;
            }
            if text != old_text {
                return Err(CiError::new(format!(
                    "{}: the requirement `{}` on {name} is not the exact pin `{old_text}`, so it cannot be moved to {target_text}",
                    manifest.path,
                    text.escape_debug()
                )));
            }
            edits.push(Edit {
                path: manifest.path.to_owned(),
                span,
                text: quoted(&new_text),
            });
        }
    }
    Ok(())
}

fn lock_edits(
    lock: &Source,
    members: &[String],
    target_text: &str,
    require: bool,
    edits: &mut Vec<Edit>,
) -> Result<(), CiError> {
    let doc = DeTable::parse(&lock.text)
        .map_err(|e| CiError::new(format!("{} is not valid TOML: {e}", lock.path)))?;
    let packages = doc
        .get_ref()
        .get("package")
        .and_then(|packages| packages.get_ref().as_array())
        .ok_or_else(|| CiError::new(format!("{} lists no package", lock.path)))?;
    for member in members {
        let mut found = false;
        for package in packages.iter().filter_map(table) {
            let named = package.get("name").and_then(|name| name.get_ref().as_str());
            if named != Some(member) || package.contains_key("source") {
                continue;
            }
            found = true;
            let version = package.get("version").ok_or_else(|| {
                CiError::new(format!("{}: package {member} has no version", lock.path))
            })?;
            if version.get_ref().as_str() != Some(target_text) {
                edits.push(Edit {
                    path: lock.path.clone(),
                    span: version.span(),
                    text: quoted(target_text),
                });
            }
        }
        if require && !found {
            return Err(CiError::new(format!(
                "{} does not list the path package {member}",
                lock.path
            )));
        }
    }
    Ok(())
}

fn apply(sources: &[&Source], mut edits: Vec<Edit>) -> Vec<Source> {
    edits.sort_by(|a, b| a.path.cmp(&b.path).then(b.span.start.cmp(&a.span.start)));
    edits.dedup_by(|a, b| a.path == b.path && a.span == b.span);
    let mut changed: BTreeMap<String, String> = BTreeMap::new();
    for edit in edits {
        let Some(source) = sources.iter().find(|source| source.path == edit.path) else {
            continue;
        };
        let text = changed
            .entry(edit.path.clone())
            .or_insert_with(|| source.text.clone());
        text.replace_range(edit.span, &edit.text);
    }
    changed
        .into_iter()
        .map(|(path, text)| Source { path, text })
        .collect()
}

pub fn align(
    lead: &Root,
    lead_package: &str,
    follower: &Root,
    members: &[String],
) -> Result<Aligned, CiError> {
    let lead_parsed = lead
        .manifests
        .iter()
        .map(Parsed::new)
        .collect::<Result<Vec<_>, _>>()?;
    let follower_parsed = follower
        .manifests
        .iter()
        .map(Parsed::new)
        .collect::<Result<Vec<_>, _>>()?;
    let lead_manifest = find(&lead_parsed, lead_package, "lead")?;
    let (target_text, _) = literal_version(lead_manifest, lead_package)?;
    let target = Version::parse(lead_package, target_text)?;
    let mut current = BTreeMap::new();
    let mut edits = Vec::new();
    for member in members {
        let manifest = find(&follower_parsed, member, "follower")?;
        let (text, span) = literal_version(manifest, member)?;
        let version = Version::parse(member, text)?;
        if version > target {
            return Err(CiError::new(format!(
                "{}: {member} is at {text}, ahead of {lead_package} at {target_text}; refusing to move it back",
                manifest.path
            )));
        }
        current.insert(member.as_str(), version);
        if version != target {
            edits.push(Edit {
                path: manifest.path.to_owned(),
                span,
                text: quoted(target_text),
            });
        }
    }
    for manifest in lead_parsed.iter().chain(&follower_parsed) {
        requirement_edits(manifest, &current, target_text, &mut edits)?;
    }
    let follower_lock = follower.lock.as_ref().ok_or_else(|| {
        CiError::new("the follower workspace has no Cargo.lock to align".to_owned())
    })?;
    lock_edits(follower_lock, members, target_text, true, &mut edits)?;
    if let Some(lock) = &lead.lock {
        lock_edits(lock, members, target_text, false, &mut edits)?;
    }
    let sources: Vec<&Source> = lead
        .manifests
        .iter()
        .chain(&follower.manifests)
        .chain(&lead.lock)
        .chain(&follower.lock)
        .collect();
    let changed = apply(&sources, edits);
    let report = if changed.is_empty() {
        format!("already aligned at {target_text}\n")
    } else {
        let mut lines = String::new();
        for source in &changed {
            lines.push_str(&["aligned ", &source.path, " to ", target_text, "\n"].concat());
        }
        lines
    };
    Ok(Aligned {
        edits: changed,
        report,
    })
}
