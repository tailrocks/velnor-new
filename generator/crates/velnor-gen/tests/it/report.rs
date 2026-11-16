// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};
use velnor_model::{HelperRelease, HelperVersion, RepoPath, StepKind, When};
use velnor_rust::{detect, plan};

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
        let cache = plan.jobs.iter().find(|job| job.id.as_str() == "cache");
        if version == "0.1.80" {
            assert!(cache.is_none());
            continue;
        }
        let cache = cache.ok_or("Cache missing")?;
        assert_eq!(cache.name.as_deref(), Some("Cache"));
        assert_eq!(cache.when, When::Finalizer);
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
            assert!(cache.needs.contains(&lane.id));
            let report = lane
                .steps
                .iter()
                .position(|step| matches!(step.kind, StepKind::ReportCaches(_)))
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
