// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, read};
use serde_json::json;
use std::collections::BTreeMap;
use std::process::Command;

use velnor_ci::report::SaveOutcome;

#[test]
fn checker_enriches_downloaded_evidence_and_publishes_each_lane_summary() -> super::ResultTest {
    let repo = Repo::new("report-check-enrichment");
    let mut lane = crate::report::lane();
    lane["job_name"] = json!("Rust build");
    lane["caches"][0]["save"] = json!("unknown");
    lane["invocations"][0]["compilations"] = json!([]);
    lane["diagnostics"] = json!(["local provenance retained"]);
    std::fs::write(repo.root().join("ci-metrics.json"), lane.to_string())?;
    let before = read(&repo);
    let api = crate::io::serve(BTreeMap::from([
        ("/repos/owner/repo/actions/runs/1/attempts/1/jobs?per_page=100&page=1".to_owned(), json!({"total_count":1,"jobs":[{"id":1,"name":"Rust build","created_at":"1970-01-01T00:00:00Z","started_at":"1970-01-01T00:00:00Z"}]}).to_string()),
        ("/repos/owner/repo/actions/caches?key=target-lock-new&per_page=100".to_owned(), json!({"total_count":1,"actions_caches":[{"key":"target-lock-new","ref":"refs/heads/main","size_in_bytes":12}]}).to_string()),
    ]));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["report", "--check"])
        .arg(repo.root())
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REPOSITORY", "owner/repo")
        .env("GITHUB_RUN_ID", "1")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_TOKEN", "fixture")
        .env("GITHUB_API_URL", api)
        .env("VELNOR_REPORT_NEEDS", "{}")
        .env("GITHUB_STEP_SUMMARY", repo.root().join("summary"))
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let metrics = read(&repo);
    assert_eq!(metrics.caches[0].save, SaveOutcome::Saved);
    assert_eq!(metrics.caches[0].bytes, Some(12));
    assert_eq!(metrics.first_cargo_seconds, Some(10));
    assert_eq!(json!(metrics.invocations), json!(before.invocations));
    assert!(
        metrics
            .diagnostics
            .iter()
            .any(|message| message == "local provenance retained")
    );
    let summary = std::fs::read_to_string(repo.root().join("summary"))?;
    for evidence in [
        "rust",
        "Saved",
        "12",
        "Some(10)",
        "local provenance retained",
    ] {
        assert!(summary.contains(evidence), "{summary}");
    }
    Ok(())
}

#[test]
fn unavailable_inventory_preserves_unknown_failed_and_unattempted_saves() -> super::ResultTest {
    save_outcomes(&[
        ("success", None, SaveOutcome::Unknown),
        ("failure", None, SaveOutcome::Failed),
        ("cancelled", None, SaveOutcome::Failed),
        ("skipped", None, SaveOutcome::NotAttempted),
        ("pr-success", None, SaveOutcome::NotAttempted),
    ])
}

#[test]
fn available_inventory_distinguishes_saved_missing_and_explicit_failure() -> super::ResultTest {
    save_outcomes(&[
        ("success", Some(true), SaveOutcome::Saved),
        ("success", Some(false), SaveOutcome::Failed),
        ("failure", Some(true), SaveOutcome::Failed),
    ])
}

fn save_outcomes(cases: &[(&str, Option<bool>, SaveOutcome)]) -> super::ResultTest {
    let repo = Repo::new("report-save-outcomes");
    for &(outcome, inventory, expected) in cases {
        let cache =
            json!([{"family":"target","root":".","restore_step":"restore","save_step":"save"}]);
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command.arg("report").current_dir(repo.root())
            .env("RUNNER_TEMP", repo.root()).env("GITHUB_JOB", "rust")
            .env("GITHUB_REF", if outcome == "pr-success" { "refs/pull/1/merge" } else { "refs/heads/main" }).env("VELNOR_REPORT_DEFAULT_BRANCH", "main")
            .env("GITHUB_REPOSITORY", "owner/repo").env("GITHUB_RUN_ID", "1")
            .env("GITHUB_RUN_ATTEMPT", "1").env("VELNOR_REPORT_CACHES", cache.to_string())
            .env("VELNOR_REPORT_STEPS", json!({"restore":{"outcome":"success","outputs":{"cache-primary-key":"key"}},"save":{"outcome":if outcome == "pr-success" { "success" } else { outcome }}}).to_string());
        inventory_api(&mut command, inventory);
        let output = command.output()?;
        assert!(output.status.success(), "{output:?}");
        let local = read(&repo);
        assert_eq!(
            local.caches[0].save,
            match outcome {
                "success" => SaveOutcome::Unknown,
                "failure" | "cancelled" => SaveOutcome::Failed,
                _ => SaveOutcome::NotAttempted,
            }
        );
        assert_eq!(local.caches[0].bytes, None);
        let mut check = checking(&repo);
        for (key, value) in command
            .get_envs()
            .filter(|(key, _)| *key == "GITHUB_TOKEN" || *key == "GITHUB_API_URL")
        {
            match value {
                Some(value) => {
                    check.env(key, value);
                }
                None => {
                    check.env_remove(key);
                }
            }
        }
        let checked = check.output()?;
        let metrics = read(&repo);
        assert_eq!(
            checked.status.success(),
            matches!(expected, SaveOutcome::Saved | SaveOutcome::NotAttempted),
            "{checked:?}"
        );
        assert_eq!(metrics.invocations.len(), local.invocations.len());
        assert_eq!(metrics.verification_errors, local.verification_errors);
        if outcome == "success" && inventory.is_none() {
            let checked = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
                .args(["report", "--check"])
                .arg(repo.root())
                .env("VELNOR_REPORT_NEEDS", "{}")
                .output()?;
            assert!(
                !checked.status.success(),
                "unknown save verification must not report clean: {checked:?}"
            );
            assert!(String::from_utf8_lossy(&checked.stderr).contains("cannot verify"));
        }
        assert_eq!(metrics.caches[0].save, expected);
        assert_eq!(
            metrics.caches[0].save == SaveOutcome::Failed,
            expected == SaveOutcome::Failed,
            "{outcome} {inventory:?}"
        );
        assert_eq!(
            !metrics.violations.is_empty(),
            expected == SaveOutcome::Failed
        );
        assert_eq!(
            metrics
                .diagnostics
                .iter()
                .any(|message| message.contains("GitHub telemetry unavailable")),
            inventory.is_none()
        );
        std::fs::remove_file(repo.root().join("ci-metrics.json"))?;
    }
    Ok(())
}

#[test]
fn cache_inventory_requires_unique_eligible_entries_and_complete_results() -> super::ResultTest {
    let repo = Repo::new("report-cache-inventory");
    let mut responses = BTreeMap::from([(
        "/repos/owner/repo/actions/runs/1/attempts/1/jobs?per_page=100&page=1".to_owned(),
        json!({"total_count":0,"jobs":[]}).to_string(),
    )]);
    let mut caches = Vec::new();
    let mut steps = serde_json::Map::new();
    for (key, refs, truncated, saving) in [
        ("unique", vec!["foreign", "main"], false, false),
        ("base", vec!["foreign", "release"], false, false),
        ("ambiguous", vec!["main", "main"], false, false),
        ("save-ambiguous", vec!["main", "main"], false, true),
        ("truncated", vec!["main"], true, false),
        ("save-truncated", vec![], true, true),
        ("missing-save", vec![], false, true),
        ("CASEKEY", vec!["main"], false, false),
    ] {
        let matched = if key == "CASEKEY" { "casekey" } else { key };
        let mut entries: Vec<_> = refs.iter().enumerate().map(|(index, branch)| {
            json!({"key":matched,"ref":format!("refs/heads/{branch}"),"version":format!("v{index}"),"size_in_bytes":10 + index})
        }).collect();
        entries.push(
            json!({"key":format!("{key}-prefix"),"ref":"refs/heads/main","size_in_bytes":99}),
        );
        if key == "missing-save" {
            entries.clear();
        }
        responses.insert(
            format!("/repos/owner/repo/actions/caches?key={matched}&per_page=100"),
            json!({"total_count":entries.len() + usize::from(truncated),"actions_caches":entries})
                .to_string(),
        );
        caches.push(json!({"family":"target","root":".","restore_step":format!("restore-{key}"),"save_step":format!("save-{key}")}));
        steps.insert(
            format!("restore-{key}"),
            json!({"outcome":"success","outputs":{"cache-primary-key":key,"cache-matched-key":matched,"cache-hit":if key == "CASEKEY" { "true" } else { "false" }}}),
        );
        if saving {
            steps.insert(format!("save-{key}"), json!({"outcome":"success"}));
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_BASE_REF", "release")
        .env("VELNOR_REPORT_DEFAULT_BRANCH", "main")
        .env("GITHUB_REPOSITORY", "owner/repo")
        .env("GITHUB_RUN_ID", "1")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_TOKEN", "fixture")
        .env("GITHUB_API_URL", crate::io::serve(responses.clone()))
        .env("VELNOR_REPORT_CACHES", json!(caches).to_string())
        .env("VELNOR_REPORT_STEPS", json!(steps).to_string())
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["report", "--check"])
        .arg(repo.root())
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_BASE_REF", "release")
        .env("VELNOR_REPORT_DEFAULT_BRANCH", "main")
        .env("GITHUB_REPOSITORY", "owner/repo")
        .env("GITHUB_RUN_ID", "1")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_TOKEN", "fixture")
        .env("GITHUB_API_URL", crate::io::serve(responses))
        .env("VELNOR_REPORT_NEEDS", "{}")
        .output()?;
    assert!(!output.status.success(), "{output:?}");
    let metrics = read(&repo);
    assert_inventory(&metrics);
    Ok(())
}

fn assert_inventory(metrics: &velnor_ci::report::LaneMetrics) {
    assert_eq!(metrics.caches.len(), 8);
    assert_eq!(metrics.caches[7].requested_key, "CASEKEY");
    assert_eq!(metrics.caches[7].matched_key.as_deref(), Some("casekey"));
    assert_eq!(metrics.caches[7].hit, velnor_ci::report::Hit::Exact);
    for (cache, expected_bytes) in
        metrics
            .caches
            .iter()
            .zip([Some(11), Some(11), None, None, None, None, None, Some(10)])
    {
        assert_eq!(cache.bytes, expected_bytes, "{}", cache.requested_key);
        assert_eq!(
            cache.save,
            if cache.requested_key.starts_with("save-") {
                SaveOutcome::Unknown
            } else if cache.requested_key == "missing-save" {
                SaveOutcome::Failed
            } else {
                SaveOutcome::NotAttempted
            },
            "{metrics:?}"
        );
        if expected_bytes.is_none() && cache.requested_key != "missing-save" {
            assert!(
                metrics
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.contains(&cache.requested_key)),
                "{:?}",
                metrics.diagnostics
            );
        }
    }
}

fn checking(repo: &Repo) -> Command {
    let mut check = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    check
        .args(["report", "--check"])
        .arg(repo.root())
        .env("GITHUB_REF", "refs/heads/main")
        .env("VELNOR_REPORT_DEFAULT_BRANCH", "main")
        .env("GITHUB_REPOSITORY", "owner/repo")
        .env("GITHUB_RUN_ID", "1")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("VELNOR_REPORT_NEEDS", "{}")
        .env("GITHUB_STEP_SUMMARY", repo.root().join("checked-summary"));
    check
}

fn inventory_api(command: &mut Command, inventory: Option<bool>) {
    if let Some(present) = inventory {
        let entries = if present {
            json!([{"key":"key","ref":"refs/heads/main","size_in_bytes":12}])
        } else {
            json!([])
        };
        let api = crate::io::serve(BTreeMap::from([
                (
                    "/repos/owner/repo/actions/runs/1/attempts/1/jobs?per_page=100&page=1"
                        .to_owned(),
                    json!({"total_count":0,"jobs":[]}).to_string(),
                ),
                (
                    "/repos/owner/repo/actions/caches?key=key&per_page=100".to_owned(),
                    json!({"total_count":entries.as_array().map_or(0, Vec::len),"actions_caches":entries}).to_string(),
                ),
            ]));
        command
            .env("GITHUB_TOKEN", "fixture")
            .env("GITHUB_API_URL", api);
    } else {
        command
            .env_remove("GITHUB_TOKEN")
            .env_remove("CARGO_TARGET_DIR");
    }
}
