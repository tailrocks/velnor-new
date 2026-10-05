//! Bounded, canonical Cargo workspace discovery through declared excludes.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use toml::Value as TomlValue;

use super::context::FreshnessContext;
use super::dependencies;
use super::glob;
use super::workspace_paths::{manifest_subject, path_text, toml_repr};

const CHECK: &str = "lock-staleness";
const MAX_DISCOVERED_WORKSPACES: usize = 4096;

pub(super) fn discover(ctx: &mut FreshnessContext) -> Vec<(PathBuf, TomlValue)> {
    discover_limited(ctx, MAX_DISCOVERED_WORKSPACES)
}

#[cfg(test)]
pub(super) fn discover_with_limit(
    ctx: &mut FreshnessContext,
    max_discovered_workspaces: usize,
) -> Vec<(PathBuf, TomlValue)> {
    discover_limited(ctx, max_discovered_workspaces)
}

fn discover_limited(
    ctx: &mut FreshnessContext,
    max_discovered_workspaces: usize,
) -> Vec<(PathBuf, TomlValue)> {
    let root_manifest = ctx.path("Cargo.toml");
    let Some(document) = dependencies::read_manifest(ctx, &root_manifest) else {
        return Vec::new();
    };
    if document
        .get("workspace")
        .and_then(TomlValue::as_table)
        .is_none()
    {
        ctx.fail_row(CHECK, "Cargo.toml", "no [workspace] table");
        return Vec::new();
    }
    let canonical_root = match fs::canonicalize(&ctx.root) {
        Ok(root) => root,
        Err(error) => {
            ctx.fail_row(
                CHECK,
                "Cargo.toml",
                &format!("cannot resolve repository root ({error})"),
            );
            return Vec::new();
        }
    };
    Discovery::new(ctx, canonical_root, document, max_discovered_workspaces).run()
}

struct Discovery<'a> {
    ctx: &'a mut FreshnessContext,
    canonical_root: PathBuf,
    seen: BTreeSet<PathBuf>,
    pending: VecDeque<(PathBuf, TomlValue)>,
    roots: Vec<(PathBuf, TomlValue)>,
    max_discovered_workspaces: usize,
    limit_exceeded: bool,
}

impl<'a> Discovery<'a> {
    fn new(
        ctx: &'a mut FreshnessContext,
        canonical_root: PathBuf,
        root_document: TomlValue,
        max_discovered_workspaces: usize,
    ) -> Self {
        Self {
            ctx,
            seen: BTreeSet::from([canonical_root.clone()]),
            canonical_root,
            pending: VecDeque::from([(PathBuf::new(), root_document)]),
            roots: Vec::new(),
            max_discovered_workspaces,
            limit_exceeded: false,
        }
    }

    fn run(mut self) -> Vec<(PathBuf, TomlValue)> {
        while let Some((relative, document)) = self.pending.pop_front() {
            let Some(workspace) = document.get("workspace").and_then(TomlValue::as_table) else {
                self.ctx
                    .fail_row(CHECK, &manifest_subject(&relative), "no [workspace] table");
                continue;
            };
            self.scan_excludes(&relative, workspace);
            self.roots.push((relative, document));
            if self.limit_exceeded {
                break;
            }
        }
        self.roots
    }

    fn scan_excludes(&mut self, relative: &Path, workspace: &toml::map::Map<String, TomlValue>) {
        let subject = manifest_subject(relative);
        let excludes = match workspace.get("exclude") {
            None => return,
            Some(TomlValue::Array(excludes)) => excludes,
            Some(_) => {
                self.ctx
                    .fail_row(CHECK, &subject, "workspace.exclude must be an array");
                return;
            }
        };
        for value in excludes {
            let Some(pattern) = value.as_str() else {
                self.ctx.fail_row(
                    CHECK,
                    &subject,
                    &format!(
                        "workspace.exclude entry is not a string: {}",
                        toml_repr(value)
                    ),
                );
                continue;
            };
            let Some(matches) = self.match_exclude(relative, &subject, pattern) else {
                continue;
            };
            for matched in matches {
                if !self.queue_workspace(&subject, &matched) {
                    return;
                }
            }
        }
    }

    fn match_exclude(
        &mut self,
        relative: &Path,
        subject: &str,
        pattern: &str,
    ) -> Option<Vec<glob::Entry>> {
        let pattern = relative.join(pattern);
        match glob::matching_entries(&self.ctx.root, &path_text(&pattern)) {
            Ok(matches) => Some(matches),
            Err(error) => {
                self.ctx.fail_row(
                    CHECK,
                    subject,
                    &format!("invalid workspace.exclude ({error})"),
                );
                None
            }
        }
    }

    fn queue_workspace(&mut self, subject: &str, matched: &glob::Entry) -> bool {
        let hit = self.ctx.root.join(&matched.path);
        let manifest = if hit.file_name().is_some_and(|name| name == "Cargo.toml") {
            hit
        } else {
            hit.join("Cargo.toml")
        };
        if !manifest.is_file() {
            return true;
        }
        let Some(directory) = manifest.parent() else {
            return true;
        };
        let canonical_directory = match fs::canonicalize(directory) {
            Ok(directory) => directory,
            Err(error) => {
                self.ctx.fail_row(
                    CHECK,
                    &path_text(&manifest),
                    &format!("cannot resolve workspace directory ({error})"),
                );
                return true;
            }
        };
        if !canonical_directory.starts_with(&self.canonical_root) {
            self.ctx.fail_row(
                CHECK,
                &path_text(&manifest),
                "workspace directory resolves outside repository root",
            );
            return true;
        }
        if self.seen.contains(&canonical_directory) {
            return true;
        }
        if self.seen.len() >= self.max_discovered_workspaces {
            self.ctx.fail_row(
                CHECK,
                subject,
                &format!(
                    "workspace discovery exceeds {} unique paths",
                    self.max_discovered_workspaces
                ),
            );
            self.limit_exceeded = true;
            return false;
        }
        self.seen.insert(canonical_directory);
        let Some(document) = dependencies::read_manifest(self.ctx, &manifest) else {
            return true;
        };
        if document
            .get("workspace")
            .and_then(TomlValue::as_table)
            .is_some()
            && let Some(relative) = manifest
                .parent()
                .and_then(|path| path.strip_prefix(&self.ctx.root).ok())
        {
            self.pending.push_back((relative.to_path_buf(), document));
        }
        true
    }
}
