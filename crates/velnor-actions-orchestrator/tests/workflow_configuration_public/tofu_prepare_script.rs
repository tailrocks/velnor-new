// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::tofu_cache::tofu_providers_prepare_step;

struct Runner {
    temp: TempDir,
}

impl Runner {
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            temp: TempDir::new()?,
        })
    }

    fn root(&self) -> Result<PathBuf, Box<dyn Error>> {
        Ok(fs::canonicalize(self.temp.path())?)
    }

    fn cache_dir(&self) -> Result<PathBuf, Box<dyn Error>> {
        Ok(self.root()?.join("velnor/tofu-cache"))
    }

    fn prepare(&self, path: &Path) -> Result<bool, Box<dyn Error>> {
        let step = tofu_providers_prepare_step(&path.to_string_lossy())?;
        let StepKind::Shell { run, env } = &step.kind else {
            return Err("prepare is not a shell step".into());
        };
        let status = Command::new(&run[0])
            .args(&run[1..])
            .envs(env)
            .env("RUNNER_TEMP", self.root()?)
            .env("PATH", "/usr/bin:/bin")
            .status()?;
        Ok(status.success())
    }
}

#[test]
fn prepare_recreates_an_empty_private_leaf() -> Result<(), Box<dyn Error>> {
    let runner = Runner::new()?;
    let leaf = runner.cache_dir()?.join("aB-_9");
    fs::create_dir_all(&leaf)?;
    fs::write(leaf.join("stale"), "x")?;
    assert!(runner.prepare(&leaf)?);
    assert!(leaf.is_dir());
    assert_eq!(fs::read_dir(&leaf)?.count(), 0);
    assert_eq!(fs::metadata(&leaf)?.permissions().mode() & 0o777, 0o700);
    Ok(())
}

#[test]
fn prepare_refuses_a_symlinked_velnor_parent() -> Result<(), Box<dyn Error>> {
    let runner = Runner::new()?;
    let elsewhere = runner.root()?.join("elsewhere");
    fs::create_dir_all(elsewhere.join("tofu-cache/slug"))?;
    symlink(&elsewhere, runner.root()?.join("velnor"))?;
    let leaf = runner.cache_dir()?.join("slug");
    fs::write(elsewhere.join("tofu-cache/slug/keep"), "x")?;
    assert!(!runner.prepare(&leaf)?);
    assert!(elsewhere.join("tofu-cache/slug/keep").exists());
    Ok(())
}

#[test]
fn prepare_refuses_a_symlinked_tofu_cache_parent() -> Result<(), Box<dyn Error>> {
    let runner = Runner::new()?;
    let elsewhere = runner.root()?.join("elsewhere");
    fs::create_dir_all(elsewhere.join("slug"))?;
    fs::create_dir_all(runner.root()?.join("velnor"))?;
    symlink(&elsewhere, runner.cache_dir()?)?;
    fs::write(elsewhere.join("slug/keep"), "x")?;
    assert!(!runner.prepare(&runner.cache_dir()?.join("slug"))?);
    assert!(elsewhere.join("slug/keep").exists());
    Ok(())
}

#[test]
fn prepare_refuses_paths_outside_the_cache_prefix_or_slug_alphabet() -> Result<(), Box<dyn Error>> {
    let runner = Runner::new()?;
    let root = runner.root()?;
    let victim = root.join("victim");
    fs::create_dir_all(&victim)?;
    fs::write(victim.join("keep"), "x")?;
    let cache = runner.cache_dir()?;
    let refused = [
        victim.clone(),
        root.join("velnor"),
        cache.clone(),
        cache.join("a/b"),
        cache.join("a.b"),
        cache.join("a b"),
        cache.join(".."),
        cache.join("../../victim"),
        PathBuf::from(format!("{}/", cache.join("slug").display())),
    ];
    for path in refused {
        assert!(!runner.prepare(&path)?, "{}", path.display());
    }
    assert!(victim.join("keep").exists());
    Ok(())
}
