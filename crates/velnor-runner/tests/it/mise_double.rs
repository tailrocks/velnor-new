// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use crate::mise_world::{World, job_fixture};

const SHELL_ADDED: [&str; 4] = ["PWD", "OLDPWD", "SHLVL", "_"];

pub(crate) struct Call {
    pub(crate) argv: String,
    pub(crate) pwd: PathBuf,
    pub(crate) ls: Vec<String>,
    pub(crate) rootls: Vec<String>,
    pub(crate) rootmode: String,
    pub(crate) homels: Vec<String>,
    pub(crate) systemls: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) files: BTreeMap<String, Vec<u8>>,
}

fn text(directory: &Path, name: &str) -> String {
    std::fs::read_to_string(directory.join(name)).unwrap_or_default()
}

fn names(directory: &Path, name: &str) -> Vec<String> {
    text(directory, name).lines().map(str::to_owned).collect()
}

impl Call {
    fn read(directory: &Path) -> Self {
        let env = text(directory, "env")
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter(|(name, _)| !SHELL_ADDED.contains(name))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        let files = std::fs::read_dir(directory.join("files")).map_or_default(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let bytes = std::fs::read(entry.path()).ok()?;
                    Some((entry.file_name().to_string_lossy().into_owned(), bytes))
                })
                .collect()
        });
        Self {
            argv: text(directory, "argv").trim_end().to_owned(),
            pwd: PathBuf::from(text(directory, "pwd").trim_end()),
            ls: names(directory, "ls"),
            rootls: names(directory, "rootls"),
            rootmode: text(directory, "rootmode"),
            homels: names(directory, "homels"),
            systemls: names(directory, "systemls"),
            env,
            files,
        }
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.pwd
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf)
    }

    pub(crate) fn env_value(&self, name: &str) -> String {
        self.env.get(name).cloned().unwrap_or_default()
    }
}

impl World {
    pub(crate) fn calls_dir(&self) -> PathBuf {
        self.root.join("calls")
    }

    pub(crate) fn configure_double(&self, settings: &[(&str, &str)]) -> Result<()> {
        self.unseal_store()?;
        let mut body = format!("CALLS={}\n", self.calls_dir().display());
        for (key, value) in settings {
            ensure!(!value.contains('\n'), "{key} holds a newline");
            let _ = writeln!(body, "{key}={value}");
        }
        let directory = self.binary_dir();
        std::fs::create_dir_all(&directory).context("create the tools store")?;
        std::fs::write(directory.join("double.conf"), body).context("write double.conf")
    }

    pub(crate) fn calls(&self) -> Vec<Call> {
        let directory = self.calls_dir();
        let mut numbered: Vec<PathBuf> = std::fs::read_dir(directory)
            .map_or_default(|entries| entries.filter_map(Result::ok).map(|e| e.path()).collect());
        numbered.sort();
        numbered.iter().map(|path| Call::read(path)).collect()
    }

    pub(crate) fn mise_calls(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .map(|call| format!("argv={}", call.argv))
            .collect()
    }

    pub(crate) fn mise_installs(&self) -> Vec<String> {
        let mut calls = self.mise_calls();
        calls.retain(|call| call.starts_with("argv=install "));
        calls
    }
}

pub(crate) fn prepared() -> Result<World> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace(true)?;
    Ok(world)
}

pub(crate) fn private_dirs(world: &World) -> Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(world.canonical_temp()?)
        .context("read the temp directory")?
        .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
        .filter(|name| name.starts_with("velnor-mise-"))
        .collect();
    names.sort();
    Ok(names)
}

pub(crate) fn git_bytes(workspace: &Path, name: &str) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["show", &format!("HEAD:{name}")])
        .output()
        .context("run git show")?;
    ensure!(output.status.success(), "git show HEAD:{name} failed");
    Ok(output.stdout)
}

pub(crate) fn cancel_during_paths(world: &World) -> Result<String> {
    let hung = world.temp().join("paths-hung");
    world.configure_double(&[("HANG_PATHS", &hung.to_string_lossy())])?;
    let job = world.only_first_step(&job_fixture("mise_action_ok"))?;
    let runner = world.start(&job, &[])?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !hung.exists() {
        if Instant::now() > deadline {
            bail!("the path probe never started");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let signalled = Command::new("kill")
        .args(["-TERM", &runner.id().to_string()])
        .status()?;
    ensure!(signalled.success(), "kill failed");
    let output = runner
        .wait_with_output()
        .context("wait for velnor-runner")?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn each_row<T: Copy + std::fmt::Debug>(
    rows: &[T],
    check: impl Fn(T) -> Result<()>,
) -> Result<()> {
    let mut failed = Vec::new();
    for row in rows {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(*row))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failed.push(format!("row {row:?}: {error:#}")),
            Err(panic) => {
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("panicked");
                failed.push(format!("row {row:?}: {message}"));
            }
        }
    }
    ensure!(failed.is_empty(), "{}", failed.join("\n"));
    Ok(())
}
