// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::js_text;
use crate::exec::ExecError;
use crate::expr::glob::{DirLinks, Hidden, PatternSet, WalkOptions};

#[derive(Debug)]
pub(super) struct Found {
    pub(super) files: Vec<(PathBuf, PathBuf)>,
    pub(super) root: PathBuf,
}

struct Root {
    lexical: PathBuf,
    real: PathBuf,
}

impl Root {
    fn of(components: &[String]) -> Self {
        let lexical = components
            .iter()
            .fold(PathBuf::from("/"), |path, name| path.join(name));
        let real = match (lexical.parent(), lexical.file_name()) {
            (Some(parent), Some(name)) => {
                fs::canonicalize(parent).map_or_else(|_| lexical.clone(), |real| real.join(name))
            }
            _ => lexical.clone(),
        };
        Self { lexical, real }
    }

    fn named(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.real)
            .map_or_else(|_| path.to_path_buf(), |inside| self.lexical.join(inside))
    }
}

pub(super) fn resolve(base: &Path, home: Option<&Path>, text: &str) -> PathBuf {
    match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => base.join(text),
    }
}

pub(super) fn with_home_prefix(home: Option<&Path>, text: &str) -> String {
    match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) => format!("{}{rest}", home.display()),
        _ => text.to_owned(),
    }
}

fn anchored(workspace: &Path, home: Option<&Path>, line: &str) -> Result<String, ExecError> {
    let mut text = line;
    let mut marker = String::new();
    while let Some(rest) = text.strip_prefix('!') {
        marker.push('!');
        text = js_text::trim_start(rest);
    }
    if text.is_empty() {
        return Err(ExecError::Invalid("pattern cannot be empty".to_owned()));
    }
    let text = text.strip_prefix("./").unwrap_or(text);
    let resolved = if text == "." {
        workspace.to_path_buf()
    } else {
        resolve(workspace, home, text)
    };
    Ok(format!("{marker}{}", resolved.display()))
}

fn common_ancestor(roots: &[PathBuf]) -> PathBuf {
    let mut shared: Vec<_> = roots[0].components().collect();
    for root in &roots[1..] {
        let length = shared
            .iter()
            .zip(root.components())
            .take_while(|(left, right)| *left == right)
            .count();
        shared.truncate(length);
    }
    shared.into_iter().collect()
}

pub(super) fn find(
    workspace: &Path,
    home: Option<&Path>,
    input: &str,
    stop: &AtomicBool,
) -> Result<Found, ExecError> {
    let lines: Vec<String> = input
        .lines()
        .map(js_text::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| anchored(workspace, home, line))
        .collect::<Result<_, _>>()?;
    let invalid = |error: &dyn std::fmt::Display| ExecError::Invalid(error.to_string());
    let patterns = PatternSet::new(Path::new("/"), home, lines.iter().map(String::as_str))
        .map_err(|error| invalid(&error))?;
    let roots: Vec<Root> = patterns.roots().iter().map(|root| Root::of(root)).collect();
    let options = WalkOptions {
        links: DirLinks::Follow,
        hidden: Hidden::Skip,
        stop: Some(stop),
    };
    let mut named = BTreeMap::new();
    for file in patterns.walk_following(Path::new("/"), options) {
        let walked = file.map_err(|error| invalid(&error))?;
        named.insert(roots[walked.root].named(&walked.path), walked.path);
    }
    if stop.load(Ordering::Relaxed) {
        return Err(ExecError::Invalid("the step was cancelled".to_owned()));
    }
    let lexical: Vec<PathBuf> = roots.into_iter().map(|root| root.lexical).collect();
    let root = match lexical.as_slice() {
        [] => workspace.to_path_buf(),
        [single] if named.len() == 1 && named.keys().next() == Some(single) => single
            .parent()
            .map_or_else(|| single.clone(), Path::to_path_buf),
        [single] => single.clone(),
        many => common_ancestor(many),
    };
    let files = named.into_iter().map(|(name, real)| (real, name)).collect();
    Ok(Found { files, root })
}
