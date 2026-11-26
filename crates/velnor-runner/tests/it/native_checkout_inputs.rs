// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, finish, git};

fn assert_rejected_without_git(rig: &Rig, ran: &Ran, scratch: &Scratch, message: &str) {
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.contains(message) || ran.stderr.contains(message),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!rig.trace().exists(), "git ran");
    assert!(!scratch.workspace().join(".git").exists());
}

#[test]
fn persist_credentials_absent_or_true_is_rejected() -> Result<()> {
    let rig = Rig::new()?;
    let url = rig.server_url();
    let jobs = [
        rig.job("checkout_no_persist", &rig.head, &url)?,
        rig.job_edit(
            "checkout_depth1",
            &rig.head,
            &url,
            &[("\"lit\": \"false\"", "\"lit\": \"true\"")],
        )?,
    ];
    for job in jobs {
        let scratch = Scratch::new()?;
        let ran = finish(rig.start(&job, &scratch)?)?;
        assert_rejected_without_git(
            &rig,
            &ran,
            &scratch,
            "actions/checkout requires persist-credentials: false",
        );
    }
    Ok(())
}

#[test]
fn malformed_commit_ids_are_rejected_before_git_runs() -> Result<()> {
    let rig = Rig::new()?;
    for sha in [
        "--upload-pack=touch pwned",
        &"g".repeat(40),
        "refs/heads/main",
    ] {
        let job = rig.job("checkout_depth1", sha, &rig.server_url())?;
        let scratch = Scratch::new()?;
        let ran = finish(rig.start(&job, &scratch)?)?;
        assert_rejected_without_git(&rig, &ran, &scratch, "trusted facts: sha");
    }
    Ok(())
}

#[test]
fn non_numeric_fetch_depth_is_rejected() -> Result<()> {
    let rig = Rig::new()?;
    let job = rig.job_edit(
        "checkout_depth1",
        &rig.head,
        &rig.server_url(),
        &[("\"lit\": \"1\"", "\"lit\": \"deep\"")],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_rejected_without_git(&rig, &ran, &scratch, "'fetch-depth' must be a number");
    Ok(())
}

#[test]
fn leftover_workspace_files_are_removed() -> Result<()> {
    let rig = Rig::new()?;
    let scratch = Scratch::new()?;
    let workspace = scratch.workspace();
    std::fs::create_dir_all(workspace.join("old/nested"))?;
    std::fs::write(workspace.join("stale.txt"), "stale")?;
    std::fs::write(workspace.join(".hidden"), "stale")?;
    std::fs::write(workspace.join("old/nested/file"), "stale")?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"])?, rig.head);
    for name in ["stale.txt", ".hidden", "old"] {
        assert!(!workspace.join(name).exists(), "{name} survived");
    }
    assert_eq!(git(&workspace, &["status", "--porcelain"])?, "");
    Ok(())
}

#[test]
fn workspace_inside_the_store_is_refused_and_the_mirror_survives() -> Result<()> {
    let rig = Rig::new()?;
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let job = rig.job("checkout_depth1", &rig.head, &rig.server_url())?;
    for workspace in [rig.mirror(), rig.mirror().join("refs/heads")] {
        let scratch = Scratch::new()?;
        let ran = finish(rig.start_at(&job, &workspace, &scratch, &[])?)?;
        assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
        assert!(
            ran.stdout.contains("must not contain each other")
                || ran.stderr.contains("must not contain each other"),
            "{}\n{}",
            ran.stdout,
            ran.stderr
        );
        let object = format!("{}^{{commit}}", rig.head);
        assert_eq!(git(&rig.mirror(), &["rev-parse", &object])?, rig.head);
    }
    Ok(())
}
