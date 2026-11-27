// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};

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

fn refused(ran: &Ran) -> bool {
    ran.code == Some(1)
        && (ran.stdout.contains("must not contain each other")
            || ran.stderr.contains("must not contain each other"))
}

#[test]
fn workspace_that_contains_the_job_temp_directory_is_refused() -> Result<()> {
    let rig = Rig::new()?;
    let scratch = Scratch::new()?;
    let workspace = scratch.workspace();
    let parent = workspace.parent().context("scratch root")?;
    let job = rig.job("checkout_depth1", &rig.head, &rig.server_url())?;
    let ran = finish(rig.start_at(&job, parent, &scratch, &[])?)?;
    assert!(refused(&ran), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        workspace
            .parent()
            .is_some_and(|root| root.join("temp").is_dir())
    );
    Ok(())
}

#[test]
fn workspace_inside_another_repositorys_mirror_is_refused_and_that_mirror_survives() -> Result<()> {
    let rig = Rig::new()?;
    let other = rig.store().join("git/_local/someone/other/1.git");
    std::fs::create_dir_all(&other)?;
    std::fs::write(other.join("HEAD"), "ref: refs/heads/main\n")?;
    let job = rig.job("checkout_depth1", &rig.head, &rig.server_url())?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start_at(&job, &other, &scratch, &[])?)?;
    assert!(refused(&ran), "{}\n{}", ran.stdout, ran.stderr);
    assert!(other.join("HEAD").is_file());
    Ok(())
}

const MARKER_STEP: &str = r#""Steps": [
    {
      "Type": "action",
      "Id": "00000000-0000-0000-0000-000000000002",
      "Name": "__run",
      "ContextName": "__run",
      "Reference": { "Type": "script" },
      "DisplayNameToken": { "type": 0, "lit": "Marker" },
      "Inputs": {
        "type": 2,
        "map": [
          {
            "key": { "type": 0, "lit": "script" },
            "value": { "type": 0, "lit": "echo step-one-marker" }
          }
        ]
      }
    },"#;

#[test]
fn an_unsupported_input_refuses_the_job_before_its_first_step() -> Result<()> {
    let rig = Rig::new()?;
    let job = rig.job_edit(
        "checkout_unsupported_ref",
        &rig.head,
        &rig.server_url(),
        &[("\"Steps\": [", MARKER_STEP)],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.has_line("unsupported: actions/checkout input 'ref' is not supported by velnor-runner"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(
        !ran.stdout.contains("step-one-marker") && !ran.stderr.contains("step-one-marker"),
        "step 1 ran\n{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!rig.trace().exists(), "git ran");
    Ok(())
}

const ENV_STEP: &str = r#""Steps": [
    {
      "Type": "action",
      "Id": "00000000-0000-0000-0000-000000000002",
      "Name": "__run",
      "ContextName": "__run",
      "Reference": { "Type": "script" },
      "DisplayNameToken": { "type": 0, "lit": "Depth" },
      "Inputs": {
        "type": 2,
        "map": [
          {
            "key": { "type": 0, "lit": "script" },
            "value": { "type": 0, "lit": "echo DEPTH=1 >> $GITHUB_ENV" }
          }
        ]
      }
    },"#;

#[test]
fn an_input_that_is_empty_at_preflight_is_not_refused() -> Result<()> {
    let rig = Rig::new()?;
    let job = rig.job_edit(
        "checkout_depth1",
        &rig.head,
        &rig.server_url(),
        &[
            ("\"Steps\": [", ENV_STEP),
            ("\"lit\": \"1\"", "\"lit\": \"${{ env.DEPTH }}\""),
            ("\"lit\": \"false\"", "\"lit\": \"${{ 'false' }}\""),
        ],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    Ok(())
}

#[test]
fn an_input_name_is_checked_even_when_its_value_does_not_evaluate() -> Result<()> {
    let rig = Rig::new()?;
    let job = rig.job_edit(
        "checkout_unsupported_ref",
        &rig.head,
        &rig.server_url(),
        &[
            ("\"Steps\": [", MARKER_STEP),
            ("\"lit\": \"refs/tags/v1\"", "\"lit\": \"${{ ( }}\""),
        ],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.has_line("unsupported: actions/checkout input 'ref' is not supported by velnor-runner"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(
        !ran.stdout.contains("step-one-marker") && !ran.stderr.contains("step-one-marker"),
        "step 1 ran\n{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!rig.trace().exists(), "git ran");
    Ok(())
}

#[test]
fn an_input_name_that_is_not_a_literal_refuses_the_job_before_its_first_step() -> Result<()> {
    let rig = Rig::new()?;
    let job = rig.job_edit(
        "checkout_unsupported_ref",
        &rig.head,
        &rig.server_url(),
        &[
            ("\"Steps\": [", MARKER_STEP),
            ("\"lit\": \"ref\"", "\"lit\": \"${{ 'ref' }}\""),
        ],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.contains("input name is not a literal"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!ran.stdout.contains("step-one-marker"), "step 1 ran");
    assert!(!rig.trace().exists(), "git ran");
    Ok(())
}

#[test]
fn a_null_fetch_depth_takes_the_default_instead_of_refusing_the_job() -> Result<()> {
    let rig = Rig::new()?;
    let job = rig.job_edit(
        "checkout_depth1",
        &rig.head,
        &rig.server_url(),
        &[("\"type\": 0,\n              \"lit\": \"1\"", "\"type\": 7")],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    Ok(())
}
