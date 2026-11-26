// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::pattern::{GlobError, Pattern, Seg, align, components_of};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

struct Entry {
    negate: bool,
    files: bool,
    segs: Vec<Seg>,
}

pub(crate) struct PatternSet {
    workspace: Vec<String>,
    entries: Vec<Entry>,
}

impl PatternSet {
    pub(crate) fn new<'a>(
        workspace: &Path,
        home: Option<&Path>,
        lines: impl Iterator<Item = &'a str>,
    ) -> Result<Self, GlobError> {
        let workspace = components_of(workspace);
        let mut entries = Vec::new();
        for line in lines {
            let pattern = Pattern::parse(line, &workspace, home)?;
            if pattern.trailing || !matches!(pattern.segs.last(), Some(Seg::GlobStar)) {
                entries.push(Entry {
                    negate: pattern.negate,
                    files: true,
                    segs: pattern.implicit_descendants(),
                });
            }
            entries.push(Entry {
                negate: pattern.negate,
                files: !pattern.trailing,
                segs: pattern.segs,
            });
        }
        Ok(Self { workspace, entries })
    }

    fn matches_file(&self, path: &[String]) -> bool {
        self.entries.iter().fold(false, |matched, entry| {
            if entry.files && align(&entry.segs, path).last() == Some(&true) {
                !entry.negate
            } else {
                matched
            }
        })
    }

    fn may_contain(&self, path: &[String]) -> bool {
        self.entries
            .iter()
            .filter(|entry| !entry.negate)
            .any(|entry| align(&entry.segs, path).iter().any(|alive| *alive))
    }

    fn roots(&self) -> Vec<Vec<String>> {
        let candidates: Vec<Vec<String>> = self
            .entries
            .iter()
            .filter(|entry| !entry.negate)
            .map(|entry| {
                entry
                    .segs
                    .iter()
                    .map_while(|seg| seg.literal().map(str::to_owned))
                    .collect()
            })
            .collect();
        let known: HashSet<&[String]> = candidates.iter().map(Vec::as_slice).collect();
        let mut included: Vec<Vec<String>> = Vec::new();
        for candidate in &candidates {
            let shadowed = (0..candidate.len()).any(|length| known.contains(&candidate[..length]));
            if !shadowed && !included.contains(candidate) {
                included.push(candidate.clone());
            }
        }
        let mut roots: Vec<Vec<String>> = Vec::new();
        for root in included {
            let clamped = if root.starts_with(&self.workspace) {
                root
            } else if self.workspace.starts_with(&root) {
                self.workspace.clone()
            } else {
                continue;
            };
            if !roots.contains(&clamped) {
                roots.push(clamped);
            }
        }
        roots
    }

    pub(crate) fn walk<'a>(&'a self, workspace: &'a Path) -> Walk<'a> {
        Walk {
            patterns: self,
            workspace,
            roots: self.roots().into_iter(),
            stack: Vec::new(),
        }
    }
}

struct Visit {
    path: PathBuf,
    comps: Vec<String>,
    kind: fs::FileType,
    matched: bool,
    partial: bool,
}

pub(crate) struct Walk<'a> {
    patterns: &'a PatternSet,
    workspace: &'a Path,
    roots: std::vec::IntoIter<Vec<String>>,
    stack: Vec<Visit>,
}

fn skip_missing<T>(result: io::Result<T>, path: &Path) -> Result<Option<T>, GlobError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(GlobError::io(path, error)),
    }
}

impl Iterator for Walk<'_> {
    type Item = Result<PathBuf, GlobError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let visit = match self.next_visit()? {
                Ok(visit) => visit,
                Err(error) => return Some(Err(error)),
            };
            match self.visit(visit) {
                Ok(Some(path)) => return Some(Ok(path)),
                Ok(None) => {}
                Err(error) => return Some(Err(error)),
            }
        }
    }
}

impl Walk<'_> {
    fn next_visit(&mut self) -> Option<Result<Visit, GlobError>> {
        while self.stack.is_empty() {
            let root = self.roots.next()?;
            match self.start(&root) {
                Ok(Some(visit)) => return Some(Ok(visit)),
                Ok(None) => {}
                Err(error) => return Some(Err(error)),
            }
        }
        self.stack.pop().map(Ok)
    }

    fn classify(&self, path: PathBuf, comps: Vec<String>, kind: fs::FileType) -> Option<Visit> {
        let matched = self.patterns.matches_file(&comps);
        let partial = self.patterns.may_contain(&comps);
        (matched || partial).then_some(Visit {
            path,
            comps,
            kind,
            matched,
            partial,
        })
    }

    fn start(&self, root: &[String]) -> Result<Option<Visit>, GlobError> {
        let relative = &root[self.patterns.workspace.len()..];
        let path = match relative.split_last() {
            None => self.workspace.to_path_buf(),
            Some((name, parents)) => {
                let parent = parents
                    .iter()
                    .fold(self.workspace.to_path_buf(), |path, name| path.join(name));
                let Some(real) = skip_missing(fs::canonicalize(&parent), &parent)? else {
                    return Ok(None);
                };
                if !real.starts_with(self.workspace) {
                    return Ok(None);
                }
                real.join(name)
            }
        };
        let Some(metadata) = skip_missing(fs::symlink_metadata(&path), &path)? else {
            return Ok(None);
        };
        Ok(self.classify(path, root.to_vec(), metadata.file_type()))
    }

    fn visit(&mut self, visit: Visit) -> Result<Option<PathBuf>, GlobError> {
        if visit.kind.is_dir() {
            if visit.partial {
                self.push_children(&visit)?;
            }
            return Ok(None);
        }
        if !visit.matched {
            return Ok(None);
        }
        if visit.kind.is_file() {
            return Ok(Some(visit.path));
        }
        if !visit.kind.is_symlink() {
            return Ok(None);
        }
        let real =
            fs::canonicalize(&visit.path).map_err(|error| GlobError::io(&visit.path, error))?;
        if !real.starts_with(self.workspace) {
            return Ok(None);
        }
        let metadata = skip_missing(fs::metadata(&real), &real)?;
        Ok(metadata.filter(fs::Metadata::is_file).map(|_| real))
    }

    fn push_children(&mut self, parent: &Visit) -> Result<(), GlobError> {
        let Some(reader) = skip_missing(fs::read_dir(&parent.path), &parent.path)? else {
            return Ok(());
        };
        let mut children = Vec::new();
        for item in reader {
            let item = item.map_err(|error| GlobError::io(&parent.path, error))?;
            let path = item.path();
            if let Some(kind) = skip_missing(item.file_type(), &path)? {
                children.push((item.file_name(), kind));
            }
        }
        children.sort_unstable_by(|left, right| right.0.cmp(&left.0));
        for (name, kind) in children {
            let mut comps = parent.comps.clone();
            comps.push(name.to_string_lossy().into_owned());
            if let Some(visit) = self.classify(parent.path.join(&name), comps, kind) {
                self.stack.push(visit);
            }
        }
        Ok(())
    }
}
