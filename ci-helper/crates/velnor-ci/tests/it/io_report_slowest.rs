// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;
use velnor_ci::report::LaneMetrics;

type ResultTest = Result<(), Box<dyn std::error::Error>>;

const PASS_LINES: [(&str, &str); 13] = [
    ("0.900", "app::bin/db db_zeta"),
    ("0.500", "app::bin/db db_open"),
    ("0.500", "app::bin/db db_close"),
    ("0.040", "app::bin/db db_ten"),
    ("0.030", "app::bin/db db_nine"),
    ("0.700", "app::bin/db db_three"),
    ("0.200", "app::bin/db db_six"),
    ("0.050", "app::bin/db db_eleven"),
    ("0.300", "app::bin/db db_five"),
    ("0.100", "app::bin/db db_seven"),
    ("0.020", "app::bin/db db_twelve"),
    ("0.010", "app::bin/db db_thirteen"),
    ("0.060", "app::bin/db db_eight"),
];

fn run_group_job(repo: &Repo) -> Result<String, Box<dyn std::error::Error>> {
    let program = repo.root().join("cargo-nextest");
    symlink("/bin/sh", &program)?;
    let mut lines = String::new();
    for (index, (seconds, test)) in PASS_LINES.iter().enumerate() {
        use std::fmt::Write;
        write!(
            lines,
            "printf '        PASS [   {seconds}s] ({:>2}/13) {test}\\n';",
            index + 1
        )?;
    }
    let script =
        format!("{lines} printf '     Summary [   3.000s] 13 tests run: 13 passed, 0 skipped\\n'");
    let summary = repo.root().join("step-summary.md");
    let run = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", program.to_str().ok_or("path")?, "-c", &script])
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust-test-database")
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(run.status.success(), "{run:?}");
    let report = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust-test-database")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("VELNOR_TESTS_RUN", "13")
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(report.status.success(), "{report:?}");
    Ok(fs::read_to_string(summary)?)
}

#[test]
fn a_test_job_summary_lists_its_ten_slowest_tests_with_ties_in_name_order() -> ResultTest {
    let repo = super::io_report::fixture("report-slowest-ten")?;
    let summary = run_group_job(&repo)?;
    let rows: Vec<&str> = summary
        .split_once("| Slowest tests | Seconds |\n|---|---|\n")
        .ok_or("no slowest table")?
        .1
        .lines()
        .take_while(|line| line.starts_with('|'))
        .collect();
    assert_eq!(
        rows,
        [
            "| app::bin/db db_zeta | 0.900 |",
            "| app::bin/db db_three | 0.700 |",
            "| app::bin/db db_close | 0.500 |",
            "| app::bin/db db_open | 0.500 |",
            "| app::bin/db db_five | 0.300 |",
            "| app::bin/db db_six | 0.200 |",
            "| app::bin/db db_seven | 0.100 |",
            "| app::bin/db db_eight | 0.060 |",
            "| app::bin/db db_eleven | 0.050 |",
            "| app::bin/db db_ten | 0.040 |",
        ]
    );
    let metrics: LaneMetrics =
        serde_json::from_slice(&fs::read(repo.root().join("ci-metrics.json"))?)?;
    assert_eq!(metrics.tests_run, Some(13));
    assert_eq!(metrics.slowest_tests.len(), 10);
    Ok(())
}
