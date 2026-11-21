// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

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
