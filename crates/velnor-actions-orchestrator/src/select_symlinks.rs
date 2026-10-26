// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use velnor_actions_mise::GitRequest;

const SYMLINK_MODE: &str = "120000";

const MAX_LINK_HOPS: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedLink {
    pub(crate) link: String,
    pub(crate) chain: Vec<String>,
    pub(crate) target: String,
}

impl ResolvedLink {
    pub(crate) fn reads(&self, path: &str) -> bool {
        self.chain.iter().any(|link| link == path)
            || path == self.target
            || path.starts_with(&format!("{}/", self.target))
            || self.target.is_empty()
    }
}

pub(crate) type LinkOutcome = Result<ResolvedLink, (String, String)>;

pub(crate) fn tracked_links(root: &Path) -> Result<Vec<LinkOutcome>, String> {
    let args = ["-s", "-z"].iter().map(OsString::from).collect();
    let output = GitRequest::ls_files(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    let mut links = Vec::new();
    for record in output.stdout.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(record).map_err(|_| "non_utf8_index_entry".to_owned())?;
        let Some((meta, path)) = text.split_once('\t') else {
            return Err("malformed_index_entry".to_owned());
        };
        if meta.split(' ').next() != Some(SYMLINK_MODE) {
            continue;
        }
        links.push(resolve_link(root, path).map_err(|problem| (path.to_owned(), problem)));
    }
    Ok(links)
}

pub(crate) fn resolve_link(root: &Path, link: &str) -> Result<ResolvedLink, String> {
    let mut chain = Vec::new();
    let mut current: Vec<String> = link.split('/').map(str::to_owned).collect();
    for _ in 0..MAX_LINK_HOPS {
        let Some((index, prefix)) = first_link_prefix(root, &current) else {
            let target = current.join("/");
            if !root.join(&target).exists() {
                return Err("dangling".to_owned());
            }
            return Ok(ResolvedLink {
                link: link.to_owned(),
                chain,
                target,
            });
        };
        let destination =
            std::fs::read_link(root.join(&prefix)).map_err(|_| "dangling".to_owned())?;
        chain.push(prefix.clone());
        let parent = Path::new(&prefix)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut next = lexical_join(&parent, &destination)?;
        next.extend(current.into_iter().skip(index + 1));
        current = next;
    }
    Err("loop".to_owned())
}

fn first_link_prefix(root: &Path, components: &[String]) -> Option<(usize, String)> {
    let mut prefix = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        prefix.push(component);
        let is_link = std::fs::symlink_metadata(root.join(&prefix))
            .is_ok_and(|meta| meta.file_type().is_symlink());
        if is_link {
            return Some((index, components[..=index].join("/")));
        }
    }
    None
}

fn lexical_join(base: &Path, destination: &Path) -> Result<Vec<String>, String> {
    if destination.is_absolute() {
        return Err("escapes_repository".to_owned());
    }
    let mut parts: Vec<String> = base
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    for component in destination.components() {
        match component {
            Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err("escapes_repository".to_owned());
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("escapes_repository".to_owned());
            }
        }
    }
    Ok(parts)
}
