// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use super::artifact_support::{Env, JOB_RUN, OLD_JOB, read_file, run_job_env, zip_of};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;

const BEFORE: &str = "echo ran > before.txt";
const HOST_OS: &str = if cfg!(target_os = "macos") {
    "macOS"
} else {
    "Linux"
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pattern_expression_over_job_start_contexts_is_refused_before_the_first_step()
-> Result<()> {
    for pattern in [
        "${{ format('@({0}|b)', runner.os) }}",
        "@(${{ runner.os }}|b)",
    ] {
        let fake = FakeResults::start().await?;
        let scratch = Scratch::new()?;
        let ran = run_job_env(
            &scratch,
            "artifact_download",
            &fake.url(),
            &[("pattern", pattern), ("path", "restored")],
            Env {
                script_before: Some(BEFORE),
                ..Env::default()
            },
        )?;
        assert_eq!(ran.code, Some(1), "{pattern}: {}", ran.stdout);
        assert!(
            ran.stdout.contains(&format!(
                "unsupported: actions/download-artifact pattern '@({HOST_OS}|b)' uses minimatch syntax"
            )),
            "{pattern}: {}",
            ran.stdout
        );
        assert!(
            !scratch.workspace().join("before.txt").exists(),
            "{pattern}"
        );
        assert_eq!(fake.calls("ListArtifacts")?.len(), 0, "{pattern}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pattern_expression_that_reads_step_state_is_checked_when_its_step_runs() -> Result<()> {
    let cases = [
        (
            "echo 'p=ci-metrics-*' >> \"$GITHUB_OUTPUT\"",
            "${{ steps.before.outputs.p || '@(a|b)' }}",
        ),
        (
            "echo 'p=metrics-*' >> \"$GITHUB_OUTPUT\"",
            "ci-${{ steps.before.outputs.p || '@(a|b)' }}",
        ),
        (
            "echo 'P=ci-metrics-*' >> \"$GITHUB_ENV\"",
            "${{ env.P || '@(a|b)' }}",
        ),
    ];
    for (script, pattern) in cases {
        let fake = FakeResults::start().await?;
        fake.seed(
            "ci-metrics-a",
            JOB_RUN,
            OLD_JOB,
            zip_of(&[("m.json", b"one")])?,
        )?;
        let scratch = Scratch::new()?;
        let ran = run_job_env(
            &scratch,
            "artifact_download",
            &fake.url(),
            &[("pattern", pattern), ("path", "restored")],
            Env {
                script_before: Some(script),
                ..Env::default()
            },
        )?;
        assert_eq!(ran.code, Some(0), "{pattern}: {}", ran.stdout);
        assert_eq!(
            read_file(scratch.workspace().join("restored/m.json"))?,
            b"one",
            "{pattern}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pattern_expression_reads_the_github_context_its_own_step_sees() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("step", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"own")])?)?;
    let scratch = Scratch::new()?;
    let ran = run_job_env(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[
            ("pattern", "${{ github.action || '@(a|b)' }}"),
            ("path", "restored"),
        ],
        Env::default(),
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        read_file(scratch.workspace().join("restored/m.json"))?,
        b"own"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_step_state_pattern_that_resolves_to_unsupported_syntax_is_refused_when_its_step_runs()
-> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("a", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"one")])?)?;
    let scratch = Scratch::new()?;
    let ran = run_job_env(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[
            ("pattern", "${{ steps.before.outputs.p }}"),
            ("path", "restored"),
        ],
        Env {
            script_before: Some("echo ran > before.txt; echo 'p=@(a|b)' >> \"$GITHUB_OUTPUT\""),
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout.contains(
            "unsupported: actions/download-artifact pattern '@(a|b)' uses minimatch syntax"
        ),
        "{}",
        ran.stdout
    );
    assert!(scratch.workspace().join("before.txt").exists());
    assert_eq!(fake.calls("ListArtifacts")?.len(), 0);
    assert!(!scratch.workspace().join("restored").exists());
    Ok(())
}
