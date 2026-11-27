// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, finish, git};

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
