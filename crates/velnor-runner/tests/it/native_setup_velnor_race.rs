// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, commit, finish, git};
use crate::native_checkout_env::wrapper_env;
use crate::native_setup_velnor_fetch::{Cold, RAN, default_branch_commit, upstream, write_action};
use crate::native_setup_velnor_ref::facts;

const MAIN: &str = r#"{"kind": "branch", "branch": "main"}"#;

fn rewrite_main(rig: &Rig) -> Result<String> {
    let work = rig.root.join("work");
    git(&work, &["reset", "-q", "--hard", &rig.head])?;
    commit(&work, "rewritten")?;
    let sha = write_action(&work)?;
    let target = upstream(rig).to_string_lossy().into_owned();
    git(
        &work,
        &["push", "-q", "--force", &target, "HEAD:refs/heads/main"],
    )?;
    Ok(sha)
}

#[test]
fn a_force_push_picked_up_by_a_concurrent_job_cannot_move_the_trusted_ref_under_a_running_check()
-> Result<()> {
    let rig = Rig::new()?;
    let pinned = default_branch_commit(&rig)?;
    let cold = Cold::new(&rig)?;
    let started = rig.root.join("check-started");
    wrapper_env(
        &rig.root,
        &format!(
            "case \" $* \" in *\" merge-base \"*) : > '{}'; sleep 4;; esac\n",
            started.display()
        ),
    )?;
    let main = facts(&rig, "push", MAIN, &rig.head)?;
    let first_scratch = Scratch::new()?;
    let first = rig.start_trusted(&cold.job(&pinned)?, &main, &first_scratch)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !started.exists() {
        if Instant::now() > deadline {
            bail!("the first job never reached its ancestry check");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let rewritten = rewrite_main(&rig)?;
    let second_scratch = Scratch::new()?;
    let second = rig.start_trusted(&cold.job(&rewritten)?, &main, &second_scratch)?;
    let first = finish(first)?;
    let second = finish(second)?;
    assert_eq!(first.code, Some(0), "{}\n{}", first.stdout, first.stderr);
    assert!(first.has_line(RAN), "{}", first.stdout);
    assert_eq!(second.code, Some(0), "{}\n{}", second.stdout, second.stderr);
    assert!(second.has_line(RAN), "{}", second.stdout);
    assert_eq!(cold.trusted_refs()?, "");
    Ok(())
}
