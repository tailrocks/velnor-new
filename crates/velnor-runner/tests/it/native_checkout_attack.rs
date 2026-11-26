// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, git};
use crate::native_checkout_sink::TlsSink;

const HOST: &str = "velnor-attack.invalid";

struct Plant {
    mirror: PathBuf,
    hostile: PathBuf,
}

fn git_init(dir: &Path, args: &[&str]) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    git(dir, &[&["init", "-q"], args].concat())?;
    Ok(())
}

fn attack(plant: impl FnOnce(&Plant) -> Result<()>) -> Result<()> {
    let rig = Rig::new()?;
    let sink = TlsSink::start(&rig.root)?;
    let mirror = rig
        .store()
        .join(format!("git/{HOST}+{}/tailrocks/velnor-new.git", sink.port));
    git_init(&mirror, &["--bare"])?;
    let hostile = rig.root.join("hostile.git");
    git_init(&hostile, &["--bare"])?;
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(hostile.join("config"))?;
    write!(
        config,
        "[http]\n\tcurloptResolve = {HOST}:{}:127.0.0.1\n",
        sink.port
    )?;
    plant(&Plant { mirror, hostile })?;
    let url = format!("https://{HOST}:{}", sink.port);
    let job = rig.job("checkout_depth1", &rig.head, &url)?;
    let scratch = Scratch::new()?;
    let runner = rig.start_env(&job, &scratch, &[("GIT_SSL_NO_VERIFY", "1")])?;
    let ran = sink.run_unvisited(runner)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    Ok(())
}

#[test]
fn planted_gitfile_in_the_mirror_does_not_redirect_git() -> Result<()> {
    attack(|plant| {
        let target = format!("gitdir: {}\n", plant.hostile.display());
        std::fs::write(plant.mirror.join(".git"), target).context("plant .git")
    })
}

#[test]
fn planted_commondir_in_the_mirror_does_not_redirect_git() -> Result<()> {
    attack(|plant| {
        let target = format!("{}\n", plant.hostile.display());
        std::fs::write(plant.mirror.join("commondir"), target).context("plant commondir")
    })
}

#[test]
fn broken_mirror_head_does_not_make_git_search_upward() -> Result<()> {
    attack(|plant| {
        let parent = plant.mirror.parent().context("mirror parent")?;
        git_init(parent, &[])?;
        std::fs::copy(plant.hostile.join("config"), parent.join(".git/config"))?;
        std::fs::write(plant.mirror.join("HEAD"), "not a ref\n").context("break HEAD")
    })
}

#[test]
fn planted_symlink_for_the_staged_config_is_not_followed() -> Result<()> {
    let rig = Rig::new()?;
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let victim = rig.root.join("victim");
    std::fs::write(&victim, "untouched\n")?;
    std::os::unix::fs::symlink(&victim, rig.mirror().join("config.velnor"))?;
    let next = rig.advance("next")?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(std::fs::read_to_string(&victim)?, "untouched\n");
    let config = rig.mirror().join("config");
    assert!(!std::fs::symlink_metadata(&config)?.file_type().is_symlink());
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    Ok(())
}
