// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Command;

use velnor_ci::report::{LaneMetrics, check};

use crate::fetch::TestResult;
use crate::io::Repo;

fn lane(name: &str, steps: &str) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let repo = Repo::empty(name);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("VELNOR_REPORT_CACHES", "[]")
        .env("VELNOR_REPORT_STEPS", steps)
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(serde_json::from_slice(&fs::read(
        repo.root().join("ci-metrics.json"),
    )?)?)
}

#[test]
fn an_unavailable_privacy_lookup_reaches_the_metrics_and_fails_the_cache_check() -> TestResult {
    let unavailable = lane(
        "report-privacy-unavailable",
        r#"{"private_repo":{"outcome":"success","outputs":{"private":"false","unavailable":"true"}}}"#,
    )?;
    assert!(unavailable.privacy_unavailable);
    assert!(
        check(&[unavailable]).is_err_and(|e| e.to_string().contains("privacy lookup")),
        "the Cache check must fail"
    );
    for (label, steps) in [
        (
            "public",
            r#"{"private_repo":{"outcome":"success","outputs":{"private":"false"}}}"#,
        ),
        ("none", "{}"),
    ] {
        let metrics = lane(&format!("report-privacy-{label}"), steps)?;
        assert!(!metrics.privacy_unavailable, "{label}");
        assert!(check(&[metrics]).is_ok(), "{label}");
    }
    Ok(())
}
