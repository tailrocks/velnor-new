// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::exec_builder::{bounded, finished, job, run, write_job};
use crate::exec_support::Scratch;

const VARIABLES: [&str; 3] = [
    "RUNNER_TOOL_CACHE",
    "RUNNER_TOOLSDIRECTORY",
    "AGENT_TOOLSDIRECTORY",
];

fn printed_tool_cache(scratch: &Scratch, environment: &[(&str, &str)]) -> Result<PathBuf> {
    let message = job(vec![run("s", "echo \"cache=$RUNNER_TOOL_CACHE\"")]);
    let path = write_job(scratch, &message)?;
    let launch_dir = scratch.work_dir();
    std::fs::create_dir_all(&launch_dir)?;
    let mut command = bounded(scratch, &path);
    command
        .current_dir(&launch_dir)
        .envs(environment.iter().copied());
    let ran = finished(command)?;
    assert_eq!(ran.code, Some(0), "{}{}", ran.stdout, ran.stderr);
    ran.stdout
        .lines()
        .find_map(|line| line.strip_prefix("cache="))
        .map(PathBuf::from)
        .context("the printed tool cache")
}

#[test]
fn exec_takes_the_first_tool_cache_variable_that_is_set() -> Result<()> {
    for (set, expected) in [
        ([0, 1, 2].as_slice(), 0),
        ([1, 2].as_slice(), 1),
        ([2].as_slice(), 2),
    ] {
        let scratch = Scratch::new()?;
        let directories: Vec<String> = VARIABLES
            .iter()
            .map(|name| scratch.work_dir().join(name).to_string_lossy().into_owned())
            .collect();
        let environment: Vec<(&str, &str)> = set
            .iter()
            .map(|index| (VARIABLES[*index], directories[*index].as_str()))
            .collect();

        let printed = printed_tool_cache(&scratch, &environment)?;

        assert_eq!(printed, Path::new(&directories[expected]), "{set:?}");
    }
    Ok(())
}

#[test]
fn exec_falls_back_to_its_own_tool_cache_when_the_first_variable_is_empty() -> Result<()> {
    let scratch = Scratch::new()?;
    let other = scratch.work_dir().join("other");
    let other = other.to_string_lossy();

    let printed = printed_tool_cache(
        &scratch,
        &[("RUNNER_TOOL_CACHE", ""), ("RUNNER_TOOLSDIRECTORY", &other)],
    )?;

    assert_eq!(printed.file_name(), Some("_tool".as_ref()), "{printed:?}");
    assert_ne!(printed, Path::new(other.as_ref()));
    Ok(())
}

#[test]
fn exec_makes_a_relative_tool_cache_absolute() -> Result<()> {
    let scratch = Scratch::new()?;

    let printed = printed_tool_cache(&scratch, &[("RUNNER_TOOL_CACHE", "relative/cache")])?;

    assert_eq!(
        printed,
        scratch.work_dir().canonicalize()?.join("relative/cache")
    );
    Ok(())
}
