// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use std::fs;
use std::process::{Command, Output};
use velnor_ci::report::LaneMetrics;

type ResultTest = Result<(), Box<dyn std::error::Error>>;

const LOG: &str = include_str!("data/sparse_hint/nextest-failure.log");

fn sparse_repo(name: &str, sparse: bool) -> Result<Repo, Box<dyn std::error::Error>> {
    let repo = Repo::new(name);
    repo.write_all(&[
        ("rust/app/src/lib.rs", "pub fn value() {}\n"),
        ("rust/app/tests/data.txt", "present\n"),
        ("docs/notes.md", "notes\n"),
        ("bench/sample.bin", "bytes\n"),
    ]);
    repo.commit();
    if sparse {
        repo.git(&["sparse-checkout", "set", "--no-cone", "/rust/"]);
    }
    let top = fs::canonicalize(repo.root())?;
    fs::write(
        repo.root().join("velnor-run.log"),
        LOG.replace("{workspace}", &top.to_string_lossy()),
    )?;
    Ok(repo)
}

fn report(repo: &Repo) -> Result<(Output, LaneMetrics), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust-test")
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let metrics = serde_json::from_slice(&fs::read(repo.root().join("ci-metrics.json"))?)?;
    Ok((output, metrics))
}

#[test]
fn report_names_the_inputs_line_for_a_tracked_file_sparse_checkout_left_out() -> ResultTest {
    let repo = sparse_repo("sparse-hint", true)?;
    assert!(!repo.root().join("docs/notes.md").exists());
    let (output, metrics) = report(&repo)?;
    let hints: Vec<&String> = metrics
        .diagnostics
        .iter()
        .filter(|text| text.contains("[[inputs]]"))
        .collect();
    assert_eq!(hints.len(), 1, "{:?}", metrics.diagnostics);
    assert!(hints[0].contains("`docs/notes.md`"), "{}", hints[0]);
    assert!(
        hints[0].contains("[[inputs]]\nglob = \"docs/notes.md\"\npackages = [\"app\"]\n"),
        "{}",
        hints[0]
    );
    assert!(hints[0].contains(".velnor/rust-ci.toml"), "{}", hints[0]);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("glob = \"docs/notes.md\""),
        "{output:?}"
    );
    Ok(())
}

#[test]
fn a_full_checkout_gets_no_inputs_hint_for_a_file_that_is_missing_anyway() -> ResultTest {
    let repo = sparse_repo("sparse-hint-full", false)?;
    let (_, metrics) = report(&repo)?;
    assert!(
        metrics
            .diagnostics
            .iter()
            .all(|text| !text.contains("[[inputs]]")),
        "{:?}",
        metrics.diagnostics
    );
    Ok(())
}

#[test]
fn the_required_job_summary_repeats_the_inputs_line_of_a_failed_lane() -> ResultTest {
    let repo = sparse_repo("sparse-hint-required", true)?;
    report(&repo)?;
    let artifacts = repo.root().join("artifacts/ci-metrics-rust-test");
    fs::create_dir_all(&artifacts)?;
    fs::copy(
        repo.root().join("ci-metrics.json"),
        artifacts.join("ci-metrics.json"),
    )?;
    let summary = repo.root().join("required-summary.md");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["report", "--check"])
        .arg(repo.root().join("artifacts"))
        .env(
            "VELNOR_REPORT_NEEDS",
            r#"{"rust-test":{"result":"failure"}}"#,
        )
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env_remove("GITHUB_TOKEN")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let text = fs::read_to_string(summary)?;
    assert!(
        text.contains("[[inputs]]\nglob = \"docs/notes.md\"\npackages = [\"app\"]\n"),
        "{text}"
    );
    Ok(())
}
