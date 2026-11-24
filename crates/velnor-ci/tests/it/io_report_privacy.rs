// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Command;

use velnor_ci::report::{LaneMetrics, check};

use crate::fetch::TestResult;
use crate::io::Repo;

fn lane(name: &str, steps: &str) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    lane_in(name, steps, None)
}

fn lane_in(
    name: &str,
    steps: &str,
    plan_mode: Option<&str>,
) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let repo = Repo::empty(name);
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    if let Some(mode) = plan_mode {
        command.env("VELNOR_REPORT_PLAN_MODE", mode);
    } else {
        command.env_remove("VELNOR_REPORT_PLAN_MODE");
    }
    let output = command
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
        ("failed", r#"{"private_repo":{"outcome":"failure"}}"#),
        (
            "failed-empty",
            r#"{"private_repo":{"outcome":"failure","outputs":{}}}"#,
        ),
    ] {
        let metrics = lane(&format!("report-privacy-{label}"), steps)?;
        assert!(metrics.privacy_unavailable, "{label}");
        assert!(check(&[metrics]).is_err(), "{label}");
    }
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

fn unavailable(name: &str, steps: &str, plan_mode: Option<&str>) -> bool {
    lane_in(name, steps, plan_mode)
        .unwrap_or_else(|error| unreachable!("{name}: {error}"))
        .privacy_unavailable
}

#[test]
fn only_an_unavailable_privacy_lookup_is_carried_into_the_metrics() {
    let step = |outputs: &str| {
        format!(r#"{{"private_repo":{{"outcome":"success","outputs":{outputs}}}}}"#)
    };
    assert!(unavailable(
        "report-privacy-flagged",
        &step(r#"{"private":"false","unavailable":"true"}"#),
        None
    ));
    assert!(!unavailable(
        "report-privacy-confirmed-public",
        &step(r#"{"private":"false"}"#),
        None
    ));
    assert!(!unavailable(
        "report-privacy-confirmed-private",
        &step(r#"{"private":"true"}"#),
        None
    ));
}

#[test]
fn a_private_repo_step_that_did_not_confirm_the_visibility_is_unavailable() {
    for (label, step) in [
        ("failed, no outputs", r#"{"outcome":"failure"}"#),
        (
            "failed, empty outputs",
            r#"{"outcome":"failure","outputs":{}}"#,
        ),
        ("cancelled", r#"{"outcome":"cancelled"}"#),
        (
            "succeeded without a verdict",
            r#"{"outcome":"success","outputs":{}}"#,
        ),
        (
            "succeeded with an unreadable verdict",
            r#"{"outcome":"success","outputs":{"private":""}}"#,
        ),
        (
            "succeeded without an outcome",
            r#"{"outputs":{"private":"false"}}"#,
        ),
    ] {
        let steps = format!(r#"{{"private_repo":{step}}}"#);
        assert!(
            unavailable(
                &format!("report-privacy-{}", label.replace([' ', ','], "-")),
                &steps,
                None
            ),
            "{label}"
        );
    }
}

const SKIPPED: &str = r#""private_repo":{"outcome":"skipped","outputs":{}}"#;

#[test]
fn a_privacy_step_skipped_by_the_lane_selection_is_not_unavailable() {
    let none = format!(
        r#"{{"plan":{{"outcome":"success","outputs":{{"rust_root_mode":"none"}}}},{SKIPPED}}}"#
    );
    assert!(!unavailable("report-skip-root-none", &none, None));
    let replanned_none = format!(
        r#"{{"plan":{{"outcome":"success","outputs":{{"rust_root_mode":"all"}}}},"replan":{{"outcome":"success","outputs":{{"rust_root_mode":"none"}}}},{SKIPPED}}}"#
    );
    assert!(!unavailable("report-skip-replanned", &replanned_none, None));
}

#[test]
fn a_privacy_step_skipped_after_a_failed_plan_is_unavailable() {
    for (label, plan) in [
        ("failed", r#"{"outcome":"failure","outputs":{}}"#),
        ("skipped", r#"{"outcome":"skipped","outputs":{}}"#),
        (
            "selected",
            r#"{"outcome":"success","outputs":{"rust_root_mode":"all"}}"#,
        ),
    ] {
        let steps = format!(r#"{{"plan":{plan},{SKIPPED}}}"#);
        assert!(
            unavailable(&format!("report-skip-after-{label}"), &steps, None),
            "{label}"
        );
    }
}

#[test]
fn the_lane_reads_its_own_mode_not_the_root_workspace_mode() {
    let steps = format!(
        r#"{{"plan":{{"outcome":"success","outputs":{{"rust_root_mode":"all","rust_tools_mode":"none"}}}},{SKIPPED}}}"#
    );
    assert!(
        !unavailable("report-own-mode-none", &steps, Some("none")),
        "an unselected workspace lane skips the lookup legitimately"
    );
    assert!(
        unavailable("report-own-mode-all", &steps, Some("all")),
        "a selected lane that skipped the lookup is unavailable"
    );
    assert!(
        unavailable("report-own-mode-empty", &steps, Some("")),
        "a plan that wrote no mode proves nothing"
    );
    assert!(
        !unavailable("report-own-mode-all-none", &steps, Some("none none")),
        "a lane over several roots is unselected when every root is"
    );
    assert!(
        unavailable("report-own-mode-one-some", &steps, Some("none some")),
        "one selected root selects the lane"
    );
}
