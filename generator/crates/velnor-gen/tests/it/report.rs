// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};
use velnor_model::{HelperRelease, HelperVersion, RepoPath, StepKind, When};
use velnor_rust::{detect, plan};

#[test]
fn required_check_cannot_duplicate_the_cache_report_display_name() -> TestResult {
    for (version, name, valid) in [
        ("0.1.80", "Cache", true),
        ("0.1.81", "Cache", false),
        ("0.1.81", "Gate", true),
    ] {
        let config = format!("required_check = \"{name}\"\n");
        let view = crate::base_save::pinned(version, &config)?;
        let result = velnor_gen::expected_with_notes(&view);
        if valid {
            let (files, _) = result?;
            assert!(!files.is_empty());
        } else {
            assert!(matches!(
                result,
                Err(velnor_gen::GenerateError::Detect(velnor_rust::DetectError::Invalid { path, .. }))
                    if path == velnor_rust::CONFIG_PATH
            ));
        }
    }
    Ok(())
}

#[test]
fn compiler_artifacts_follow_report_support_and_nextest_version() -> TestResult {
    for helper in ["0.1.80", "0.1.81"] {
        for nextest in ["0.9.122", "0.9.123"] {
            for shape in ["one-job", "lint-test", "groups"] {
                let config = format!("shape = \"{shape}\"\n");
                let mut view = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
                for (path, bytes) in &mut view.files {
                    if path.as_str() == ".velnor/rust-ci.lock" {
                        *bytes = String::from_utf8(bytes.clone())?
                            .replace("0.1.79", helper)
                            .into_bytes();
                    } else if path.as_str() == "mise.toml" {
                        *bytes = String::from_utf8(bytes.clone())?
                            .replace("0.9.151", nextest)
                            .into_bytes();
                    } else if path.as_str() == "mise.lock" {
                        *bytes = String::from_utf8(bytes.clone())?
                        .replace("example.invalid", "fixtures.invalid")
                        .replace(
                            "[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"1\"",
                            &format!("[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"{nextest}\""),
                        )
                        .into_bytes();
                    }
                }
                let parsed: serde_json::Value = serde_saphyr::from_str(&workflow(&view)?)?;
                let mut cargo = 0;
                let mut nextest_commands = 0;
                for job in parsed["jobs"].as_object().ok_or("jobs")?.values() {
                    for step in job["steps"].as_array().ok_or("steps")? {
                        let Some(run) = step["run"].as_str() else {
                            continue;
                        };
                        let compiler = run.contains("cargo-clippy clippy ")
                            || run.contains(" cargo check ")
                            || run.contains(" cargo build ")
                            || run.contains(" cargo doc ")
                            || run.contains(" cargo test ");
                        let builds_tests = run.contains("cargo-nextest nextest ")
                            && !run.contains("nextest run --archive-file");
                        let flag = "--message-format=json-render-diagnostics";
                        assert_eq!(run.contains(flag), compiler && helper == "0.1.81", "{run}");
                        assert_eq!(
                            run.contains("--cargo-message-format=json-render-diagnostics"),
                            builds_tests && helper == "0.1.81" && nextest == "0.9.123",
                            "{run}"
                        );
                        if compiler {
                            cargo += 1;
                            if let Some(separator) = run.find(" -- -D") {
                                assert!(
                                    helper == "0.1.80"
                                        || run.find(flag).is_some_and(|at| at < separator)
                                );
                            }
                        }
                        nextest_commands += usize::from(builds_tests);
                    }
                }
                assert!(cargo > 0 && nextest_commands > 0);
            }
        }
    }
    Ok(())
}

#[test]
fn reporting_is_gated_by_helper_version_and_cache_blocks_only_when_configured() -> TestResult {
    for (version, blocks) in [("0.1.80", false), ("0.1.81", false), ("0.1.81", true)] {
        let config = format!("cache_check_blocks = {blocks}\n");
        let view = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
        let mut facts = detect(&view)?;
        let helper = facts.helper.as_ref().ok_or("helper")?;
        facts.helper = Some(HelperRelease::new(
            HelperVersion::new(version)?,
            helper.action().clone(),
            helper.digests().clone(),
        )?);
        let plan = plan(&facts);
        let rendered = velnor_render::render(&plan).map_err(|errors| format!("{errors:?}"))?;
        let parsed: serde_json::Value =
            serde_saphyr::from_str(rendered.values().next().ok_or("workflow")?)?;
        let cache = plan.jobs.iter().find(|job| job.id.as_str() == "cache");
        if version == "0.1.80" {
            assert!(cache.is_none());
            assert!(
                parsed["jobs"]
                    .as_object()
                    .ok_or("jobs")?
                    .values()
                    .all(|job| job["permissions"]["actions"].is_null())
            );
            continue;
        }
        let cache = cache.ok_or("Cache missing")?;
        assert_eq!(cache.name.as_deref(), Some("Cache"));
        assert_eq!(cache.when, When::Finalizer);
        assert!(parsed["jobs"]["cache"]["permissions"]["actions"].is_null());
        assert_eq!(
            parsed["jobs"]["rust-trim"]["permissions"]["actions"],
            "write"
        );
        let required = plan
            .jobs
            .iter()
            .find(|job| job.id.as_str() == "required")
            .ok_or("Required")?;
        assert_eq!(required.needs.contains(&cache.id), blocks);
        for lane in plan.jobs.iter().filter(|job| {
            job.steps
                .iter()
                .any(|step| matches!(step.kind, StepKind::Cargo(_)))
        }) {
            assert_eq!(
                parsed["jobs"][lane.id.as_str()]["permissions"]["actions"],
                "read"
            );
            assert!(cache.needs.contains(&lane.id));
            let report = lane
                .steps
                .iter()
                .position(|step| matches!(step.kind, StepKind::ReportCaches { .. }))
                .ok_or("report")?;
            assert!(
                lane.steps.iter().skip(report + 1).any(
                    |step| step.kind == StepKind::UploadMetrics && step.when == When::Finalizer
                )
            );
            assert!(
                !lane
                    .steps
                    .iter()
                    .skip(report + 1)
                    .any(|step| matches!(step.kind, StepKind::CacheSave(_)))
            );
        }
    }
    Ok(())
}

#[test]
fn report_cache_descriptors_preserve_literal_roots_without_github_expressions() -> TestResult {
    for root in ["root${{ secrets.EXAMPLE }}", "root{\"quoted\"}"] {
        let mut view = view(&[&format!("{root}/Cargo.toml")], None)?;
        let lock = RepoPath::new(".velnor/rust-ci.lock")?;
        let bytes = view.files.get_mut(&lock).ok_or("lock")?;
        *bytes = String::from_utf8(bytes.clone())?
            .replace("0.1.79", "0.1.81")
            .into_bytes();
        let config = RepoPath::new(".cargo/config.toml")?;
        view.tracked_paths.insert(config.clone());
        view.files
            .insert(config, b"[build]\ntarget-dir = \"shared\"\n".to_vec());
        let parsed: serde_json::Value = serde_saphyr::from_str(&workflow(&view)?)?;
        let mut reported_root = false;
        for job in parsed["jobs"].as_object().ok_or("jobs")?.values() {
            for report in job["steps"]
                .as_array()
                .ok_or("steps")?
                .iter()
                .filter(|step| step["run"] == "velnor-ci report")
            {
                let raw = report["env"]["VELNOR_REPORT_CACHES"]
                    .as_str()
                    .ok_or("cache descriptors")?;
                assert!(!raw.contains("${{"), "{raw}");
                assert!(!raw.contains("fromJSON"), "{raw}");
                let caches: serde_json::Value = serde_json::from_str(raw)?;
                for cache in caches.as_array().ok_or("caches")? {
                    assert_eq!(cache["requested_key"], "");
                    assert_eq!(cache["save"], "not_attempted");
                    assert!(cache.get("save_failed").is_none());
                    assert!(cache.get("lock_prefix").is_none());
                    if !cache["root"].is_null() {
                        assert_eq!(cache["root"], root);
                        reported_root = true;
                    }
                }
            }
        }
        assert!(reported_root);
    }
    Ok(())
}

#[test]
fn rendered_reports_keep_selection_but_run_after_failed_restores() -> TestResult {
    let mut view = view(&["Cargo.toml"], None)?;
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    let bytes = view.files.get_mut(&lock).ok_or("lock")?;
    let old = String::from_utf8(bytes.clone())?;
    *bytes = old.replace("0.1.79", "0.1.81").into_bytes();
    let text = workflow(&view)?;
    let parsed: serde_json::Value = serde_saphyr::from_str(&text)?;
    let job = &parsed["jobs"]["rust"];
    let steps = job["steps"].as_array().ok_or("steps")?;
    let report = steps
        .iter()
        .find(|step| step["run"] == "velnor-ci report")
        .ok_or("report")?;
    let condition = report["if"].as_str().ok_or("condition")?;
    assert!(condition.contains("!cancelled()"));
    assert!(!condition.contains("restored.outcome"));
    assert_eq!(report["env"]["VELNOR_REPORT_STEPS"], "${{ toJSON(steps) }}");
    assert!(
        steps
            .iter()
            .filter(|step| step["uses"]
                .as_str()
                .is_some_and(|action| action.starts_with("actions/cache/")))
            .all(|step| step["id"].is_string())
    );
    for tests in [true, false] {
        for metadata in view.rust_metadata.values_mut() {
            for member in &mut metadata.members {
                for target in &mut member.targets {
                    target.tests = tests;
                }
            }
        }
        let parsed: serde_json::Value = serde_saphyr::from_str(&workflow(&view)?)?;
        for (id, job) in parsed["jobs"].as_object().ok_or("jobs")? {
            let steps = job["steps"].as_array().ok_or("steps")?;
            let Some(report) = steps.iter().find(|step| step["run"] == "velnor-ci report") else {
                continue;
            };
            let reports_tests = steps.iter().any(|step| step["id"] == "test");
            assert_eq!(
                report["env"]["VELNOR_TESTS_RUN"].as_str(),
                reports_tests.then_some("${{ steps.test.outputs.tests_run }}"),
                "{id} tests={tests}"
            );
            let upload = steps
                .iter()
                .find(|step| step["name"] == "Upload cache metrics")
                .ok_or("metrics upload")?;
            assert_eq!(upload["with"]["overwrite"], true, "{id} tests={tests}");
        }
    }
    Ok(())
}
