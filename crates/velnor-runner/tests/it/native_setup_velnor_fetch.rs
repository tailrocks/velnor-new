// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::exec_support::{Ran, Scratch};
use crate::mise_world::{SHA_PLACEHOLDER, World, job_fixture};
use crate::native_checkout::{Rig, finish, git};
use crate::native_checkout_mirror::exits_cancelled_while_the_mirror_lock_is_held;

const ACTION: &str = "name: pinned
description: pinned
inputs:
  version:
    required: true
  sha256-linux-x64:
    default: ''
  sha256-macos-arm64:
    default: ''
runs:
  using: composite
  steps:
    - name: Run the pinned action
      shell: bash
      run: echo pinned-action-ran
";
const FACTS_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";
const SERVER_URL: &str = "file:///velnor-test";
const RAN: &str = "pinned-action-ran";

fn write_action(work: &Path) -> Result<String> {
    let directory = work.join("actions/setup-velnor");
    std::fs::create_dir_all(&directory).context("create the action directory")?;
    std::fs::write(directory.join("action.yml"), ACTION).context("write the action")?;
    git(work, &["add", "actions"])?;
    git(work, &["commit", "-q", "-m", "action"])?;
    git(work, &["rev-parse", "HEAD"])
}

fn upstream(rig: &Rig) -> PathBuf {
    rig.root.join("up/tailrocks/velnor-new.git")
}

fn default_branch_commit(rig: &Rig) -> Result<String> {
    let work = rig.root.join("work");
    let sha = write_action(&work)?;
    git(
        &work,
        &["push", "-q", &upstream(rig).to_string_lossy(), "main"],
    )?;
    Ok(sha)
}

fn impostor_commit(rig: &Rig, advertised: bool) -> Result<String> {
    let work = rig.root.join("impostor");
    std::fs::create_dir_all(&work).context("create the impostor work tree")?;
    git(&work, &["init", "-q", "-b", "main"])?;
    let sha = write_action(&work)?;
    let upstream = upstream(rig);
    git(
        &work,
        &[
            "push",
            "-q",
            &upstream.to_string_lossy(),
            "HEAD:refs/pull/9/head",
        ],
    )?;
    if !advertised {
        git(&upstream, &["update-ref", "-d", "refs/pull/9/head"])?;
    }
    git(&upstream, &["cat-file", "-e", &format!("{sha}^{{commit}}")])?;
    Ok(sha)
}

struct Cold<'a> {
    rig: &'a Rig,
    world: World,
}

impl Cold<'_> {
    fn new(rig: &Rig) -> Result<Cold<'_>> {
        Ok(Cold {
            rig,
            world: World::new()?,
        })
    }

    fn job(&self, reference: &str) -> Result<PathBuf> {
        self.job_at(reference, &self.rig.head)
    }

    fn job_at(&self, reference: &str, commit: &str) -> Result<PathBuf> {
        let url = self.rig.server_url();
        self.world.with_text(
            &job_fixture("setup_velnor_real_action"),
            &[
                (SHA_PLACEHOLDER, reference),
                (SERVER_URL, url.as_str()),
                (FACTS_SHA, commit),
            ],
        )
    }

    fn run(&self, reference: &str) -> Result<Ran> {
        self.run_job(&self.job(reference)?)
    }

    fn run_job(&self, job: &Path) -> Result<Ran> {
        let scratch = Scratch::new()?;
        finish(self.rig.start(job, &scratch)?)
    }

    fn mirror_has(&self, sha: &str) -> bool {
        git(
            &self.rig.mirror(),
            &["cat-file", "-e", &format!("{sha}^{{commit}}")],
        )
        .is_ok()
    }

    fn mirror_refs(&self) -> Result<String> {
        git(&self.rig.mirror(), &["for-each-ref", "--format=%(refname)"])
    }
}

#[test]
fn setup_velnor_without_checkout_fetches_pinned_commit() -> Result<()> {
    let rig = Rig::new()?;
    let pinned = default_branch_commit(&rig)?;
    let cold = Cold::new(&rig)?;
    assert!(!rig.mirror().exists());
    let first = cold.run(&pinned)?;
    assert_eq!(first.code, Some(0), "{}\n{}", first.stdout, first.stderr);
    assert!(first.has_line(RAN), "{}", first.stdout);
    assert!(cold.mirror_has(&pinned));
    assert_eq!(rig.fetches(), 1);
    assert_eq!(cold.mirror_refs()?, "refs/heads/main");
    let next = rig.advance("third")?;
    let second = cold.run_job(&cold.job_at(&pinned, &next)?)?;
    assert_eq!(second.code, Some(0), "{}\n{}", second.stdout, second.stderr);
    assert_eq!(rig.fetches(), 1);
    Ok(())
}

#[test]
fn an_impostor_pin_is_never_fetched_and_the_step_fails() -> Result<()> {
    for advertised in [true, false] {
        let rig = Rig::new()?;
        let impostor = impostor_commit(&rig, advertised)?;
        let cold = Cold::new(&rig)?;
        let ran = cold.run(&impostor)?;
        assert_eq!(ran.code, Some(1), "{advertised}: {}", ran.stdout);
        assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
        let unreachable =
            format!("setup-velnor: the pin {impostor} is not reachable from the trusted refs");
        assert!(
            ran.lines().iter().any(|line| line.contains(&unreachable)),
            "{advertised}: {}",
            ran.stdout
        );
        assert!(!ran.has_line(RAN), "{}", ran.stdout);
        assert!(!cold.mirror_has(&impostor), "{advertised}");
        assert_eq!(rig.fetches(), 1, "{advertised}");
        assert_eq!(cold.mirror_refs()?, "refs/heads/main", "{advertised}");
    }
    Ok(())
}

#[test]
fn a_pin_that_is_not_40_hex_fails_with_zero_fetches() -> Result<()> {
    let rig = Rig::new()?;
    let pinned = default_branch_commit(&rig)?;
    let cold = Cold::new(&rig)?;
    for reference in [
        "main",
        "refs/pull/9/head",
        &pinned[..39],
        &format!("{pinned}0"),
        &format!("{pinned}{pinned}")[..64],
        &"g".repeat(40),
    ] {
        let ran = cold.run(reference)?;
        let message = format!("setup-velnor reference '{reference}' is not a 40 digit commit sha");
        assert!(ran.has_line(&message), "{reference}\n{}", ran.stdout);
        assert_eq!(ran.code, Some(1));
        assert_eq!(rig.fetches(), 0, "{reference}");
        assert!(!rig.store().join("git").exists(), "{reference}");
    }
    Ok(())
}

#[test]
fn a_hangup_interrupt_or_terminate_while_another_process_holds_the_mirror_lock_exits_cancelled_in_setup_velnor()
-> Result<()> {
    let rig = Rig::new()?;
    let pinned = default_branch_commit(&rig)?;
    let cold = Cold::new(&rig)?;
    let job = cold.job(&pinned)?;
    exits_cancelled_while_the_mirror_lock_is_held(&rig, &job)
}
