// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use rustix::io::Errno;
use rustix::process::{Pid, test_kill_process};

use super::artifact_support::{
    Env, JOB_RUN, OLD_JOB, read_file, run_job, run_job_env, spawn_job, zip_of,
};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;
use super::support::Reply;

const UNSUPPORTED: &str = "uses minimatch syntax that is not supported by velnor-runner";
const NAMES: [&str; 8] = [
    "ci-a", "ci-b", "ci-10", "Ci-a", "ci.a", "a-b", ".hidden", "ab",
];

fn shown(pattern: &str) -> String {
    pattern.chars().take(40).collect()
}

async fn selected(pattern: &str) -> Result<Vec<String>> {
    let fake = FakeResults::start().await?;
    for name in NAMES {
        fake.seed(
            name,
            JOB_RUN,
            OLD_JOB,
            zip_of(&[("m.json", name.as_bytes())])?,
        )?;
    }
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", pattern), ("path", "got")],
    )?;
    assert_eq!(ran.code, Some(0), "{}: {}", shown(pattern), ran.stdout);
    let mut names: Vec<String> = ran
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .filter_map(|line| line.split(" (ID: ").next())
        .map(str::to_owned)
        .collect();
    names.sort();
    Ok(names)
}

async fn refused_before_the_first_step(pattern: &str) -> Result<String> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let ran = run_job_env(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", pattern)],
        Env {
            script_before: Some("echo ran > before.txt"),
            ..Env::default()
        },
    )?;
    let label = shown(pattern);
    assert_eq!(ran.code, Some(1), "{label}: {}", ran.stdout);
    assert!(!scratch.workspace().join("before.txt").exists(), "{label}");
    assert_eq!(fake.calls("ListArtifacts")?.len(), 0, "{label}");
    Ok(ran.stdout)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn patterns_outside_the_verified_subset_are_refused_before_the_first_step() -> Result<()> {
    for pattern in [
        "build-{1..3}",
        "x{a}",
        "[[:alpha:]]x",
        "ci-[",
        "\\@(linux)",
        "${ci-a,ci-b}",
        "{a,b",
        "ci-a}",
        "**",
        "x**",
        "*{a,}*",
    ] {
        let stdout = refused_before_the_first_step(pattern).await?;
        let expected =
            format!("unsupported: actions/download-artifact pattern '{pattern}' {UNSUPPORTED}");
        assert!(stdout.contains(&expected), "{pattern}: {stdout}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pattern_expanding_beyond_the_cap_is_refused_at_once() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let running = spawn_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", &"{a,b}".repeat(64))],
        &[],
    )?;
    running.wait_for_line("expands to more than 256 brace alternatives")?;
    let finished = running.finish("the runner to refuse the pattern")?;
    assert_eq!(finished.code, Some(1), "{}", finished.stdout);
    assert_eq!(fake.calls("ListArtifacts")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pattern_expanding_beyond_the_token_bound_is_refused_before_it_is_expanded() -> Result<()>
{
    let wide = |literals: usize| format!("{}{}", "a".repeat(literals), "{a,b}".repeat(8));
    let stdout = refused_before_the_first_step(&wide(65_000)).await?;
    assert!(
        stdout.contains("expands to more than 65536 tokens"),
        "{}",
        &stdout[..stdout.len().min(300)]
    );
    let stdout = refused_before_the_first_step(&wide(249)).await?;
    assert!(
        stdout.contains("expands to more than 65536 tokens"),
        "{}",
        &stdout[..stdout.len().min(300)]
    );
    assert_eq!(selected(&wide(248)).await?, Vec::<String>::new());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pattern_nested_beyond_the_depth_limit_is_refused_without_a_crash() -> Result<()> {
    let stdout = refused_before_the_first_step(&"{".repeat(30_000)).await?;
    assert!(
        stdout.contains("nests brace lists deeper than 32 levels"),
        "{}",
        &stdout[..stdout.len().min(300)]
    );
    let nested = |depth: usize| format!("{}b{}", "{a,".repeat(depth), "}".repeat(depth));
    let deepest = nested(32);
    assert_eq!(selected(&deepest).await?, Vec::<String>::new());
    let stdout = refused_before_the_first_step(&nested(33)).await?;
    assert!(
        stdout.contains("nests brace lists deeper than 32 levels"),
        "{stdout}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pattern_longer_than_minimatch_allows_is_refused() -> Result<()> {
    let stdout = refused_before_the_first_step(&"a".repeat(65_537)).await?;
    assert!(
        stdout.contains("pattern is longer than 65536 UTF-16 code units"),
        "{}",
        &stdout[..stdout.len().min(300)]
    );
    let astral = "\u{1f600}".repeat(32_769);
    let stdout = refused_before_the_first_step(&astral).await?;
    assert!(
        stdout.contains("pattern is longer than 65536 UTF-16 code units"),
        "{}",
        &stdout[..stdout.len().min(300)]
    );
    assert_eq!(selected(&"a".repeat(65_536)).await?, Vec::<String>::new());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pattern_at_the_cap_selects_what_it_names() -> Result<()> {
    let fake = FakeResults::start().await?;
    for name in ["abababab", "bbbbbbbb", "abababac"] {
        fake.seed(name, JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"x")])?)?;
    }
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", &"{a,b}".repeat(8)), ("path", "got")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        ran.has_line("Total of 2 artifact(s) downloaded"),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sigterm_while_list_artifacts_is_pending_stops_the_step_promptly() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.script("ListArtifacts", vec![Reply::stall()])?;
    let scratch = Scratch::new()?;
    let running = spawn_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[],
        &[("VELNOR_CANCEL_GRACE_MS", "900000")],
    )?;
    running.wait_for_line("step 1 start: actions/download-artifact")?;
    running.terminate()?;
    let done = running.finish("the runner to stop")?;
    assert!(
        done.stdout.lines().any(|line| line == "step 1 cancelled"),
        "{}",
        done.stdout
    );
    assert!(!done.stdout.contains("Total of"), "{}", done.stdout);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_class_with_65000_items_over_256_alternatives_selects_its_matches() -> Result<()> {
    let fake = FakeResults::start().await?;
    for name in NAMES {
        fake.seed(name, JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"x")])?)?;
    }
    let long = |tail: &str| format!("{}{tail}", "x".repeat(240));
    let wanted = [long("aaaaaaaaa"), long("baaaaaaaa")];
    let unwanted = long("caaaaaaaa");
    for name in wanted.iter().chain([&unwanted]) {
        fake.seed(
            name,
            JOB_RUN,
            OLD_JOB,
            zip_of(&[("m.json", name.as_bytes())])?,
        )?;
    }
    let scratch = Scratch::new()?;
    let pattern = format!("*[{}]{}", "ab".repeat(32_500), "{a,b}".repeat(8));
    let running = spawn_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", &pattern), ("path", "got")],
        &[],
    )?;
    let done = running.finish("a class-heavy pattern to finish")?;
    assert_eq!(done.code, Some(0), "{}", done.stdout);
    assert!(
        done.stdout
            .lines()
            .any(|line| line == "Total of 2 artifact(s) downloaded"),
        "{}",
        done.stdout
    );
    for name in &wanted {
        let path = scratch.workspace().join(format!("got/{name}/m.json"));
        assert_eq!(read_file(path)?, name.as_bytes());
    }
    assert!(!scratch.workspace().join(format!("got/{unwanted}")).exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_class_naming_the_dot_twice_is_not_a_dotted_literal() -> Result<()> {
    for pattern in ["[..]hidden", "[...]hidden", "[.a.]hidden"] {
        assert_eq!(selected(pattern).await?, Vec::<String>::new(), "{pattern}");
    }
    for pattern in ["[.]hidden", "[.-.]hidden"] {
        assert_eq!(selected(pattern).await?, [".hidden"], "{pattern}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_running_runner_is_killed_and_reaped_when_its_handle_is_dropped() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.script("ListArtifacts", vec![Reply::stall()])?;
    let scratch = Scratch::new()?;
    let running = spawn_job(&scratch, "artifact_download", &fake.url(), &[], &[])?;
    running.wait_for_line("step 1 start: actions/download-artifact")?;
    let runner = Pid::from_raw(i32::try_from(running.pid())?).context("the runner pid")?;
    drop(running);
    assert_eq!(
        test_kill_process(runner),
        Err(Errno::SRCH),
        "the runner must be gone and reaped once its handle is dropped"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supported_patterns_select_what_minimatch_selects() -> Result<()> {
    let cases: [(&str, &[&str]); 10] = [
        ("ci-a", &["ci-a"]),
        ("ci-?", &["ci-a", "ci-b"]),
        ("ci-[ab]", &["ci-a", "ci-b"]),
        ("ci-[!a]", &["ci-b"]),
        ("ci-[0-9]*", &["ci-10"]),
        ("ci{-,.}a", &["ci-a", "ci.a"]),
        ("ci-{a,{b,}}", &["ci-a", "ci-b"]),
        ("*-*", &["Ci-a", "a-b", "ci-10", "ci-a", "ci-b"]),
        ("{.hidden,ab}", &[".hidden", "ab"]),
        ("a?", &["ab"]),
    ];
    for (pattern, expected) in cases {
        assert_eq!(selected(pattern).await?, expected, "{pattern}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_star_does_not_select_a_name_with_a_slash() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("x/y", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"slash")])?)?;
    fake.seed("fine", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"fine")])?)?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", "*"), ("path", "got")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        ran.has_line("Total of 1 artifact(s) downloaded"),
        "{}",
        ran.stdout
    );
    assert_eq!(read_file(scratch.workspace().join("got/m.json"))?, b"fine");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pattern_is_trimmed_as_the_toolkit_trims_it() -> Result<()> {
    for pattern in ["ci-a\u{85}", "\u{85}ci-a"] {
        assert_eq!(
            selected(pattern).await?,
            Vec::<String>::new(),
            "{pattern:?}"
        );
    }
    assert_eq!(selected("\u{feff}ci-a\u{feff}").await?, ["ci-a"]);
    Ok(())
}
