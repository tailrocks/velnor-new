// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::base_save::{jobs, steps};
use crate::facts::TestResult;
use crate::pipeline::workflow;
use velnor_model::RepoPath;
use velnor_rust::CONFIG_PATH;

const TOKENS: [&str; 3] = ["GITHUB_TOKEN", "github.token", "secrets."];

fn runs_repository_code(step: &str) -> bool {
    step.lines()
        .any(|line| line.trim().starts_with("run:") && line.contains("cargo"))
}

#[test]
fn no_step_holds_a_token_after_cargo_ran_repository_code_in_the_same_job() -> TestResult {
    for config in ["", "shape = \"lint-test\"\n", "shape = \"groups\"\n"] {
        for (id, block) in jobs("0.1.81", config)? {
            let mut executed = false;
            for step in steps(&block) {
                assert!(
                    !executed || !TOKENS.iter().any(|token| step.contains(token)),
                    "{id} gives a token to a step after repository code ran:\n{step}"
                );
                executed |= runs_repository_code(step);
            }
        }
    }
    Ok(())
}

#[test]
fn report_api_tokens_are_confined_to_a_released_helper_without_repository_code() -> TestResult {
    for version in ["0.1.80", "0.1.81"] {
        let mut view = crate::golden::load(&crate::golden::samples().join("helper-source"))?;
        let lock = view
            .files
            .get_mut(&RepoPath::new(".velnor/rust-ci.lock")?)
            .ok_or("lock")?;
        *lock = String::from_utf8(lock.clone())?
            .replace("0.1.81", version)
            .into_bytes();
        if version == "0.1.80" {
            let config = view.files.get_mut(&CONFIG_PATH).ok_or("config")?;
            config.extend_from_slice(b"full_run = \"off\"\n");
        }
        let parsed: serde_json::Value = serde_saphyr::from_str(&workflow(&view)?)?;
        let jobs = parsed["jobs"].as_object().ok_or("jobs")?;
        let cache = &jobs["cache"];
        let cache_steps = cache["steps"].as_array().ok_or("cache steps")?;
        assert!(cache_steps.iter().all(|step| {
            !step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/checkout@"))
                && !step["run"]
                    .as_str()
                    .is_some_and(|run| run.contains("cargo"))
        }));
        let check = cache_steps
            .iter()
            .find(|step| step["name"] == "Check cache waste")
            .ok_or("check")?;
        assert_eq!(
            check["env"]["GITHUB_TOKEN"].is_string(),
            version == "0.1.81"
        );
        assert_eq!(
            cache["permissions"]["actions"].as_str(),
            (version == "0.1.81").then_some("read")
        );
        assert_eq!(
            cache_steps.iter().any(|step| step["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("Install velnor-ci"))),
            version == "0.1.81"
        );
        assert_eq!(
            cache_steps
                .iter()
                .any(|step| step["name"] == "Download velnor-ci"),
            version == "0.1.80"
        );
        for (id, job) in jobs {
            for report in job["steps"]
                .as_array()
                .ok_or("steps")?
                .iter()
                .filter(|step| step["run"] == "velnor-ci report")
            {
                assert!(report["env"]["GITHUB_TOKEN"].is_null(), "{id}: {report}");
                assert_eq!(
                    job["permissions"]["actions"].as_str(),
                    (version == "0.1.81" && job["outputs"]["base_save"].is_string())
                        .then_some("read"),
                    "{id}: {job}"
                );
                assert_eq!(report["env"]["VELNOR_REPORT_STEPS"], "${{ toJSON(steps) }}");
                assert!(
                    job["steps"]
                        .as_array()
                        .ok_or("steps")?
                        .iter()
                        .any(|step| step["name"] == "Upload cache metrics")
                );
            }
        }
    }
    Ok(())
}

#[test]
fn the_upload_verification_runs_in_the_cache_job_that_runs_no_repository_code() -> TestResult {
    let all = jobs("0.1.81", "")?;
    let block = |wanted: &str| {
        all.iter()
            .find(|(id, _)| id == wanted)
            .map(|(_, block)| block.as_str())
            .ok_or("missing job")
    };
    assert!(!block("rust")?.contains("cache verify"));
    let trim = block("rust-trim")?;
    assert!(trim.contains("velnor-ci cache verify"), "{trim}");
    assert!(
        !trim.contains("checkout") && !trim.contains("cargo"),
        "{trim}"
    );
    Ok(())
}
