// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use velnor_model::RepoPath;
use velnor_rust::{NESTED_ROOTS_FOLDER, resolve};

use super::Workspace;
use crate::error::CiError;

pub(in crate::io) fn apart(workspaces: &[Workspace], shared: &[Vec<usize>]) -> Result<(), CiError> {
    let directories: Vec<Vec<(&str, PathBuf)>> = workspaces
        .iter()
        .map(|w| {
            w.directories()
                .into_iter()
                .enumerate()
                .map(|(index, directory)| {
                    let kind = if index == 0 { "target" } else { "build" };
                    resolve(&directory)
                        .map(|resolved| (kind, resolved))
                        .map_err(|error| {
                            CiError::new(format!("cannot resolve {}: {error}", directory.display()))
                        })
                })
                .collect::<Result<_, _>>()
        })
        .collect::<Result<_, _>>()?;
    for (outer, holders) in directories.iter().enumerate() {
        for (inner, helds) in directories.iter().enumerate() {
            if outer == inner {
                continue;
            }
            for (holder_kind, holder) in holders {
                for (held_kind, held) in helds {
                    if !held.starts_with(holder)
                        || held.starts_with(holder.join(NESTED_ROOTS_FOLDER))
                    {
                        continue;
                    }
                    let grouped = held == holder
                        && shared
                            .iter()
                            .any(|group| group.contains(&outer) && group.contains(&inner));
                    if !grouped {
                        return Err(CiError::new(format!(
                            "the {holder_kind} directory {} of the Cargo root {} holds the {held_kind} directory {} of the Cargo root {}, so their units cannot be told apart; give each Cargo root its own target and build directories, outside the others",
                            holder.display(),
                            workspaces[outer].root.display(),
                            held.display(),
                            workspaces[inner].root.display()
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

pub(in crate::io) fn outside_tracked(
    workspaces: &[Workspace],
    repository: &Path,
    tracked: &BTreeSet<RepoPath>,
) -> Result<(), CiError> {
    let resolved = |path: &Path| {
        resolve(path)
            .map_err(|error| CiError::new(format!("cannot resolve {}: {error}", path.display())))
    };
    let repository = resolved(repository)?;
    for workspace in workspaces {
        let Some(build) = workspace.directories().into_iter().nth(1) else {
            continue;
        };
        let build = resolved(&build)?;
        let inside = build.strip_prefix(&repository).ok();
        let held = tracked.iter().find(|input| match inside {
            Some(relative) => {
                Path::new(input.as_str()).starts_with(relative)
                    || relative.starts_with(input.as_str())
            }
            None => repository.starts_with(&build),
        });
        if let Some(input) = held {
            return Err(CiError::new(format!(
                "the build directory {} of the Cargo root {} holds the tracked input {input}, so its units cannot be told apart from checkout content; set a build directory outside every tracked path",
                build.display(),
                workspace.root.display()
            )));
        }
    }
    Ok(())
}
