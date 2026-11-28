// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};

use super::args::CleanupArgs;
use super::flatten::Wedged;
use super::layout::{cgroup_of, root_of, run_dir_of, stage_dir_of, work_dir_of};
use super::{CGROUP_ROOT, GEN_ROOT, JOBS_ROOT, SANDBOX_ROOTS, WEDGED_EXIT, cgroup, dirs, net};

fn children(directory: &Path) -> Result<Vec<PathBuf>> {
    match std::fs::read_dir(directory) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .with_context(|| format!("list {}", directory.display()))
            })
            .collect(),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).with_context(|| format!("list {}", directory.display())),
    }
}

fn job_cgroups() -> Result<Vec<PathBuf>> {
    Ok(children(Path::new(CGROUP_ROOT))?
        .into_iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("job-"))
        })
        .collect())
}

#[derive(Debug)]
pub(super) struct Left {
    paths: Vec<String>,
    wedged: bool,
}

impl std::fmt::Display for Left {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "could not remove {}", self.paths.join(", "))
    }
}

impl std::error::Error for Left {}

fn remove_all(paths: &[PathBuf]) -> Result<()> {
    let mut left = Left {
        paths: Vec::new(),
        wedged: false,
    };
    for path in paths {
        if let Err(error) = dirs::remove(path) {
            eprintln!("velnor sandbox cleanup: {error:#}");
            left.wedged |= error.downcast_ref::<Wedged>().is_some();
            left.paths.push(path.display().to_string());
        }
    }
    if left.paths.is_empty() {
        Ok(())
    } else {
        Err(left.into())
    }
}

pub(super) fn reservation(reservation: u64) -> Result<()> {
    cgroup::kill_and_remove(&cgroup_of(reservation))?;
    net::down(reservation)?;
    remove_all(&[
        work_dir_of(reservation),
        stage_dir_of(reservation),
        root_of(reservation),
        run_dir_of(reservation),
    ])
}

fn everything() -> Result<()> {
    for path in job_cgroups()? {
        cgroup::kill_and_remove(&path)?;
    }
    net::down_all()?;
    let mut directories = Vec::new();
    for parent in [
        Path::new(GEN_ROOT).join("work"),
        Path::new(GEN_ROOT).join("stage"),
        PathBuf::from(SANDBOX_ROOTS),
        PathBuf::from(JOBS_ROOT),
    ] {
        directories.extend(children(&parent)?);
    }
    remove_all(&directories)
}

pub fn run(args: &CleanupArgs) -> Result<ExitCode> {
    let outcome = match args.reservation {
        Some(id) => reservation(id),
        None => everything(),
    };
    match outcome {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(error) if error.downcast_ref::<Left>().is_some_and(|left| left.wedged) => {
            eprintln!("velnor sandbox cleanup: {error:#}");
            Ok(ExitCode::from(WEDGED_EXIT))
        }
        Err(error) => Err(error),
    }
}
