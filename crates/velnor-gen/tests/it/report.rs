// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};
use velnor_model::{HelperRelease, HelperVersion, RepoPath, StepKind, When};
use velnor_rust::{detect, plan};

#[test]
fn required_check_cannot_duplicate_the_cache_report_display_name() -> TestResult {
    for (version, name, valid) in [("0.1.81", "Cache", false), ("0.1.81", "Gate", true)] {
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
    for helper in ["0.1.81"] {
        for nextest in ["0.9.122", "0.9.123"] {
            for shape in ["one-job", "lint-test", "groups"] {
                let config = format!("shape = \"{shape}\"\n");
                let mut view = view(&["Cargo.toml", "tools/Cargo.toml"], Some(config.as_bytes()))?;
                for (path, bytes) in &mut view.files {
                    if path.as_str() == ".velnor/rust-ci.lock" {
                        *bytes = String::from_utf8(bytes.clone())?
                            .replace("0.1.81", helper)
                            .into_bytes();
                    } else if path.as_str() == "mise.toml" {
                        *bytes = String::from_utf8(bytes.clone())?
                            .replace("0.9.151", nextest)
                            .into_bytes();
                    } else if path.as_str() == "mise.lock" {
                        *bytes = String::from_utf8(bytes.clone())?
                        .replace("example.invalid", "fixtures.invalid")
                        .replace(
                            "[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"0.9.151\"",
                            &format!("[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"{nextest}\""),
                        )
                        .into_bytes();
                    }
                }
                let parsed: serde_json::Value =
                    serde_saphyr::from_str(&crate::archive::plain_invocations(&workflow(&view)?))?;
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
                                assert!(run.find(flag).is_some_and(|at| at < separator));
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
    for (version, blocks) in [("0.1.81", false), ("0.1.81", true)] {
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
        let parsed: serde_json::Value = serde_saphyr::from_str(
            &crate::archive::plain_invocations(rendered.values().next().ok_or("workflow")?),
        )?;
        assert_metrics_policy(&parsed)?;
        let cache = plan.jobs.iter().find(|job| job.id.as_str() == "cache");
        let cache = cache.ok_or("Cache missing")?;
        assert_unified_cache_verdict(&parsed)?;
        assert_eq!(cache.name.as_deref(), Some("Cache"));
        assert_eq!(cache.when, When::Finalizer);
        assert_eq!(parsed["jobs"]["cache"]["permissions"]["actions"], "read");
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
                parsed["jobs"][lane.id.as_str()]["permissions"]["actions"].as_str(),
                parsed["jobs"][lane.id.as_str()]["outputs"]["base_save"]
                    .is_string()
                    .then_some("read")
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

fn assert_unified_cache_verdict(workflow: &serde_json::Value) -> TestResult {
    let jobs = workflow["jobs"].as_object().ok_or("jobs")?;
    assert_eq!(
        jobs.values().filter(|job| job["name"] == "Cache").count(),
        1
    );
    let cache = &jobs["cache"];
    let steps = cache["steps"].as_array().ok_or("cache steps")?;
    let waste = steps
        .iter()
        .position(|step| step["name"] == "Check cache waste")
        .ok_or("waste check")?;
    assert_ne!(steps[waste]["continue-on-error"], true);
    let needs = cache["needs"].as_array().ok_or("cache needs")?;
    assert!(needs.iter().any(|need| need == "rust-trim"));
    for (id, job) in jobs
        .iter()
        .filter(|(_, job)| job["outputs"]["target_key"].is_string())
    {
        assert!(needs.iter().any(|need| need == id));
        assert!(
            job["steps"]
                .as_array()
                .ok_or("producer steps")?
                .iter()
                .any(|step| step["run"] == "velnor-ci report")
        );
    }
    let maintenance = &jobs["rust-trim"];
    assert_eq!(maintenance["name"], "Cache maintenance");
    assert_eq!(maintenance["permissions"]["actions"], "write");
    let steps = maintenance["steps"].as_array().ok_or("maintenance steps")?;
    assert!(steps.iter().any(|step| {
        step["run"]
            .as_str()
            .is_some_and(|run| run.starts_with("velnor-ci cache trim "))
    }));
    assert!(
        steps
            .iter()
            .any(|step| step["run"] == "velnor-ci cache check --success \"$CACHE_OK\"")
    );
    Ok(())
}

#[test]
fn report_cache_descriptors_preserve_literal_roots_without_github_expressions() -> TestResult {
    for root in ["root${{ secrets.EXAMPLE }}", "root{\"quoted\"}"] {
        let mut view = view(&[&format!("{root}/Cargo.toml")], None)?;
        let lock = RepoPath::new(".velnor/rust-ci.lock")?;
        let bytes = view.files.get_mut(&lock).ok_or("lock")?;
        *bytes = String::from_utf8(bytes.clone())?.into_bytes();
        let config = RepoPath::new(".cargo/config.toml")?;
        view.tracked_paths.insert(config.clone());
        view.files
            .insert(config, b"[build]\ntarget-dir = \"shared\"\n".to_vec());
        let parsed: serde_json::Value =
            serde_saphyr::from_str(&crate::archive::plain_invocations(&workflow(&view)?))?;
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
                assert_eq!(report["env"]["VELNOR_REPORT_STEPS"], "${{ toJSON(steps) }}");
                assert!(!raw.contains("${{"), "{raw}");
                assert!(!raw.contains("fromJSON"), "{raw}");
                let caches: serde_json::Value = serde_json::from_str(raw)?;
                for cache in caches.as_array().ok_or("caches")? {
                    assert_eq!(
                        cache
                            .as_object()
                            .ok_or("cache plan")?
                            .keys()
                            .map(String::as_str)
                            .collect::<Vec<_>>(),
                        ["family", "restore_step", "root", "save_step"]
                    );
                    for key in ["family", "restore_step", "save_step"] {
                        assert!(cache[key].as_str().is_some_and(|value| !value.is_empty()));
                    }
                    if cache["family"] == "sources" {
                        assert!(cache["root"].is_null(), "{cache}");
                    } else if !cache["root"].is_null() {
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
    *bytes = old.into_bytes();
    let text = crate::archive::plain_invocations(&workflow(&view)?);
    let parsed: serde_json::Value = serde_saphyr::from_str(&text)?;
    let job = &parsed["jobs"]["rust"];
    let steps = job["steps"].as_array().ok_or("steps")?;
    let report = steps
        .iter()
        .find(|step| step["run"] == "velnor-ci report")
        .ok_or("report")?;
    let condition = report["if"].as_str().ok_or("condition")?;
    assert!(condition.contains("!cancelled()"));
    assert!(!condition.contains("restored.outcome") && !condition.contains("success()"));
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
        let parsed: serde_json::Value =
            serde_saphyr::from_str(&crate::archive::plain_invocations(&workflow(&view)?))?;
        for (id, job) in parsed["jobs"].as_object().ok_or("jobs")? {
            let steps = job["steps"].as_array().ok_or("steps")?;
            let Some(report) = steps.iter().find(|step| step["run"] == "velnor-ci report") else {
                continue;
            };
            let reports_tests = steps.iter().any(|step| step["id"] == "test");
            assert_eq!(report["id"], "report_cache_metrics");
            assert_eq!(report["continue-on-error"], true);
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
            assert_eq!(
                upload["if"],
                "${{ (steps.report_cache_metrics.outcome == 'success') && (!cancelled()) }}"
            );
            assert_eq!(upload["continue-on-error"], true);
        }
    }
    Ok(())
}

fn assert_metrics_policy(workflow: &serde_json::Value) -> TestResult {
    for job in workflow["jobs"].as_object().ok_or("jobs")?.values() {
        let steps = job["steps"].as_array().ok_or("steps")?;
        for step in steps {
            let producer = matches!(
                step["name"].as_str(),
                Some("Report cache telemetry" | "Upload cache metrics")
            );
            if step["name"] == "Report cache telemetry" {
                assert!(step["env"]["GITHUB_TOKEN"].is_null());
            }
            let verified_save = step["uses"]
                == "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
                && matches!(
                    step["name"].as_str(),
                    Some("Save the target cache" | "Save the sources cache")
                );
            let verification = step["run"] == "velnor-ci cache verify --key \"$CACHE_KEY\""
                || step["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with("Fetch the sources of root "));
            assert_eq!(
                step["continue-on-error"].as_bool().unwrap_or(false),
                producer || verified_save || verification,
                "{step}"
            );
        }
    }
    Ok(())
}
