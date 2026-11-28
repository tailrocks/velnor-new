// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

mod acl;
#[cfg(test)]
mod tests;

const ADMIN_GID: u32 = 80;
const MAX_LINKS: u32 = 40;
const GROUP_WRITE: u32 = 0o020;
const OTHER_WRITE: u32 = 0o002;
const STICKY: u32 = 0o1000;
const LAUNCH_ENVIRONMENT: [&str; 2] = ["VELNOR_ORB", "VELNOR_RUNNER_BINARY"];

#[derive(Debug, Clone, Copy)]
struct Policy {
    installer: u32,
    admin_gid: u32,
}

enum Step {
    Root,
    Parent,
    Name(OsString),
}

#[must_use]
pub fn untrusted_path(path: &Path, installer: u32) -> Option<PathBuf> {
    find(
        path,
        Policy {
            installer,
            admin_gid: ADMIN_GID,
        },
    )
}

#[must_use]
pub fn untrusted_launch_environment(installer: u32) -> Option<PathBuf> {
    LAUNCH_ENVIRONMENT
        .iter()
        .filter_map(std::env::var_os)
        .find_map(|value| untrusted_path(Path::new(&value), installer))
}

fn steps(path: &Path) -> VecDeque<Step> {
    path.components()
        .filter_map(|component| match component {
            Component::RootDir => Some(Step::Root),
            Component::ParentDir => Some(Step::Parent),
            Component::Normal(name) => Some(Step::Name(name.to_owned())),
            Component::CurDir | Component::Prefix(_) => None,
        })
        .collect()
}

fn find(path: &Path, policy: Policy) -> Option<PathBuf> {
    if !path.is_absolute() {
        return Some(path.to_path_buf());
    }
    let mut pending = steps(path);
    let mut at = PathBuf::from("/");
    if !policy.node_is_trusted(&at) {
        return Some(at);
    }
    let mut links = 0;
    while let Some(step) = pending.pop_front() {
        match step {
            Step::Root => at = PathBuf::from("/"),
            Step::Parent => {
                at.pop();
            }
            Step::Name(name) => {
                let entry = at.join(name);
                let Ok(metadata) = std::fs::symlink_metadata(&entry) else {
                    return Some(entry);
                };
                if !policy.holder_admits(&at, &metadata) {
                    return Some(entry);
                }
                if metadata.is_symlink() {
                    links += 1;
                    let target = std::fs::read_link(&entry)
                        .ok()
                        .filter(|_| links <= MAX_LINKS);
                    let Some(target) = target else {
                        return Some(entry);
                    };
                    for step in steps(&target).into_iter().rev() {
                        pending.push_front(step);
                    }
                } else if policy.node_is_trusted(&entry) {
                    at = entry;
                } else {
                    return Some(entry);
                }
            }
        }
    }
    let is_file = std::fs::symlink_metadata(&at).is_ok_and(|metadata| metadata.is_file());
    if is_file { None } else { Some(at) }
}

impl Policy {
    fn owner_is_trusted(self, uid: u32) -> bool {
        uid == 0 || uid == self.installer
    }

    fn node_is_trusted(self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|metadata| {
            (metadata.is_file() || metadata.is_dir())
                && self.owner_is_trusted(metadata.uid())
                && self.mode_is_closed(&metadata)
                && !acl::grants_write(path)
        })
    }

    fn mode_is_closed(self, metadata: &Metadata) -> bool {
        let mode = metadata.mode();
        let sticky_root = metadata.is_dir() && metadata.uid() == 0 && mode & STICKY != 0;
        sticky_root
            || (mode & OTHER_WRITE == 0
                && (mode & GROUP_WRITE == 0 || metadata.gid() == self.admin_gid))
    }

    fn holder_admits(self, holder: &Path, entry: &Metadata) -> bool {
        let sticky_root = std::fs::symlink_metadata(holder)
            .is_ok_and(|metadata| metadata.uid() == 0 && metadata.mode() & STICKY != 0);
        !sticky_root || self.owner_is_trusted(entry.uid())
    }
}
