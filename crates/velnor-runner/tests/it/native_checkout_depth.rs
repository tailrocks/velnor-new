// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, finish, git};
use crate::native_checkout_env::wrapper_env;

fn history_after(fixture: &str, edit: Option<&str>) -> Result<(String, bool)> {
    let rig = Rig::new()?;
    let edits: Vec<(&str, &str)> = edit.map(|to| ("\"lit\": \"1\"", to)).into_iter().collect();
    let job = rig.job_edit(fixture, &rig.merge, &rig.server_url(), &edits)?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let workspace = scratch.workspace();
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"])?, rig.merge);
    let count = git(&workspace, &["rev-list", "--count", "HEAD"])?;
    let has_parent = git(&workspace, &["rev-parse", "--verify", "-q", "HEAD^"]).is_ok();
    Ok((count, has_parent))
}

#[test]
fn depth_one_has_no_history() -> Result<()> {
    assert_eq!(
        history_after("checkout_depth1", None)?,
        ("1".to_owned(), false)
    );
    Ok(())
}

#[test]
fn depth_zero_has_the_full_history() -> Result<()> {
    assert_eq!(
        history_after("checkout_depth1", Some("\"lit\": \"0\""))?,
        ("4".to_owned(), true)
    );
    Ok(())
}

#[test]
fn depth_two_cuts_after_the_parents() -> Result<()> {
    assert_eq!(
        history_after("checkout_depth1", Some("\"lit\": \"2\""))?,
        ("3".to_owned(), true)
    );
    Ok(())
}

#[test]
fn empty_depth_means_one() -> Result<()> {
    assert_eq!(
        history_after("checkout_depth1", Some("\"lit\": \"\""))?,
        ("1".to_owned(), false)
    );
    Ok(())
}

#[test]
fn a_padded_depth_is_trimmed_like_core_get_input() -> Result<()> {
    assert_eq!(
        history_after("checkout_depth1", Some("\"lit\": \" 2 \""))?,
        ("3".to_owned(), true)
    );
    Ok(())
}

#[test]
fn a_blank_depth_means_one() -> Result<()> {
    assert_eq!(
        history_after("checkout_depth1", Some("\"lit\": \"  \""))?,
        ("1".to_owned(), false)
    );
    Ok(())
}

fn warm_mirror(rig: &Rig) -> Result<()> {
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    Ok(())
}

fn depth_two_succeeds(rig: &Rig) -> Result<Ran> {
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth2", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let workspace = scratch.workspace();
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"])?, rig.head);
    assert_eq!(git(&workspace, &["rev-list", "--count", "HEAD"])?, "2");
    Ok(ran)
}

fn broken_ref_line(rig: &Rig) -> String {
    format!(
        "printf '%s\\n' {} > '{}'",
        "1".repeat(40),
        rig.mirror().join("refs/heads/broken").display()
    )
}

#[test]
fn a_broken_mirror_ref_is_removed_before_a_depth_two_checkout() -> Result<()> {
    let rig = Rig::new()?;
    warm_mirror(&rig)?;
    let broken = rig.mirror().join("refs/heads/broken");
    std::fs::write(&broken, format!("{}\n", "1".repeat(40)))?;
    let ran = depth_two_succeeds(&rig)?;
    assert!(
        ran.stdout.contains("removed refs/heads/broken"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!broken.exists());
    Ok(())
}

#[test]
fn a_ref_that_breaks_during_the_depth_two_fetch_does_not_fail_the_checkout() -> Result<()> {
    let rig = Rig::new()?;
    warm_mirror(&rig)?;
    wrapper_env(
        &rig.root,
        &format!(
            "case \" $* \" in *\" --depth=2 \"*) {};; esac\n",
            broken_ref_line(&rig)
        ),
    )?;
    depth_two_succeeds(&rig)?;
    assert!(rig.mirror().join("refs/heads/broken").exists());
    Ok(())
}

#[test]
fn a_depth_two_checkout_starts_no_background_maintenance() -> Result<()> {
    let rig = Rig::new()?;
    depth_two_succeeds(&rig)?;
    assert_eq!(rig.commands("maintenance"), 0);
    Ok(())
}
