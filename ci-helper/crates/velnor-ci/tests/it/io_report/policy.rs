// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, read};
use serde_json::json;
use std::collections::BTreeMap;
use std::process::Command;

use velnor_ci::report::SaveOutcome;

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
        for name in ["GITHUB_TOKEN", "GITHUB_API_URL"] {
            if let Some(value) = command
                .get_envs()
                .find_map(|(key, value)| (key == name).then_some(value).flatten())
            {
                check.env(name, value);
            } else {
                check.env_remove(name);
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

#[test]
fn configured_external_targets_have_portable_compilation_identity() -> super::ResultTest {
    normalization("configured", "target/", true)
}

#[test]
fn default_target_paths_have_portable_compilation_identity() -> super::ResultTest {
    normalization("checkout", "target/", true)
}

#[test]
fn unknown_external_targets_have_unknown_compilation_identity() -> super::ResultTest {
    normalization("unknown", "/external/", false)
}

fn normalization(case: &str, prefix: &str, duplicate: bool) -> super::ResultTest {
    let repo = super::fixture("report-normalized-checkout")?;
    let root = std::fs::canonicalize(repo.root())?;
    let external = "/external/build";
    let filename = if case == "checkout" {
        format!("{}/target/debug/deps/libdemo-hash.rmeta", root.display())
    } else {
        format!("{external}/debug/deps/libdemo-hash.rmeta")
    };
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["root"] = json!(root);
    invocation["argv"] = json!(["cargo", "check", "--manifest-path", root.join("Cargo.toml")]);
    invocation["compilations"] = json!([]);
    invocation["progress"] = json!([]);
    invocation["target_directory"] = if case == "configured" {
        json!(external)
    } else {
        json!(null)
    };
    invocation["artifacts"] = json!([{
        "package_id":format!("path+file://{}#demo@0.1.0", root.display()), "manifest_path":root.join("Cargo.toml"),
        "target":{"name":"demo","kind":["lib"],"crate_types":["lib"],"src_path":root.join("src/lib.rs")},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":[],"filenames":[filename],"fresh":false
    }]);
    let raw = format!("{invocation}\n");
    let path = repo.root().join("velnor-invocations.jsonl");
    std::fs::write(&path, &raw)?;
    let metrics = super::report(
        &repo,
        &json!([]),
        r#"{"restore":{"outcome":"skipped","outputs":{}}}"#,
    )?;
    let observed = &metrics.invocations[0];
    let artifact = &observed.artifacts[0];
    assert_eq!(observed.root, ".");
    assert_eq!(artifact.manifest_path, "checkout/Cargo.toml");
    assert_eq!(artifact.target.src_path, "checkout/src/lib.rs");
    assert_eq!(artifact.package_id, "path+file://checkout#demo@0.1.0");
    let expected = if duplicate {
        format!("{prefix}debug/deps/libdemo-hash.rmeta")
    } else {
        filename
    };
    assert_eq!(artifact.filenames, [expected], "{case}");
    assert_eq!(json!(observed.argv), invocation["argv"]);
    assert_eq!(
        json!(observed.target_directory),
        invocation["target_directory"]
    );
    assert_eq!(std::fs::read_to_string(path)?, raw);
    assert_eq!(
        metrics
            .diagnostics
            .iter()
            .any(|message| message.contains("identity unknown for 1")),
        !duplicate,
        "{case}: {:?}",
        metrics.diagnostics
    );
    let mut other = metrics.clone();
    "other".clone_into(&mut other.lane);
    let checked = velnor_ci::report::check(&[metrics, other]);
    assert_eq!(checked.is_err(), duplicate, "{case}: {checked:?}");
    if duplicate {
        assert!(
            checked
                .err()
                .ok_or("duplicate compilation")?
                .to_string()
                .contains("identical observed compilation unit")
        );
    }
    Ok(())
}

#[test]
fn documenting_and_nonfresh_artifacts_corroborate_real_progress() -> super::ResultTest {
    corroboration(false)
}

#[test]
fn fresh_artifact_progress_does_not_claim_a_new_compilation() -> super::ResultTest {
    corroboration(true)
}

fn corroboration(fresh: bool) -> super::ResultTest {
    let repo = super::fixture("report-corroborated-progress")?;
    let root = std::fs::canonicalize(repo.root())?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["root"] = json!(root);
    invocation["compilations"] = json!([]);
    invocation["progress"] = json!([
        {"kind":"Compiling","package":"demo","version":"0.1.0","location":"/demo"},
        {"kind":"Checking","package":"demo","version":"0.1.0","location":"/demo"},
        {"kind":"Documenting","package":"demo","version":"0.1.0","location":"/demo"},
        {"kind":"Compiling","package":"lookalike","version":"9.0.0","location":null}
    ]);
    if !fresh {
        invocation["progress"].as_array_mut().ok_or("progress")?.extend([
        json!({"kind":"Compiling","package":"evil","version":"0.1.0","location":"/demo"}),
        json!({"kind":"Checking","package":"demo","version":"9.0.0","location":"/demo"}),
        json!({"kind":"Compiling","package":"demo","version":"0.1.0","location":"/elsewhere"})]);
    }
    let artifact = json!({
        "package_id":"path+file:///demo#0.1.0", "manifest_path":"/demo/Cargo.toml",
        "target":{"name":"lab_demo","kind":["lib"],"crate_types":["lib"],"src_path":"checkout/src/lib.rs"},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":[],"filenames":[],"fresh":false
    });
    let mut artifacts = Vec::new();
    for filename in [
        "target/debug/deps/liblab_demo-hash.rmeta",
        "target/debug/deps/liblab_demo-hash.rlib",
        "target/doc/lab_demo/index.html",
    ] {
        let mut event = artifact.clone();
        event["filenames"] = json!([filename]);
        artifacts.push(event);
    }
    if fresh {
        let mut event = artifact;
        event["package_id"] = json!("registry+https://example.invalid/index#lookalike@9.0.0");
        event["target"]["name"] = json!("lookalike");
        event["fresh"] = json!(true);
        artifacts.push(event);
    }
    invocation["artifacts"] = json!(artifacts);
    std::fs::write(
        repo.root().join("velnor-invocations.jsonl"),
        format!("{invocation}\n"),
    )?;
    let metrics = super::report(
        &repo,
        &json!([]),
        r#"{"restore":{"outcome":"skipped","outputs":{}}}"#,
    )?;
    assert_eq!(
        metrics.invocations[0]
            .progress
            .as_ref()
            .ok_or("progress")?
            .len(),
        if fresh { 4 } else { 7 }
    );
    assert_eq!(metrics.invocations[0].compilations.len(), 3);
    assert!(
        metrics.invocations[0]
            .compilations
            .iter()
            .all(|event| event.package == "demo")
    );
    assert_eq!(metrics.violations, [] as [String; 0]);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["report", "--check"])
        .arg(repo.root())
        .env("VELNOR_REPORT_NEEDS", "{}")
        .output()?;
    assert_eq!(output.status.success(), fresh, "{output:?}");
    Ok(())
}
