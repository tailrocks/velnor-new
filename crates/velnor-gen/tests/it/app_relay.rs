// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use serde_json::Value;
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, private_repo};
use crate::release_family_fixture::{TOOLS_LIKE, generate};
use crate::release_fixture::{APP, LOCK, TOOLS, release_view};
use crate::release_unguarded::expected_release;

mod lint;

const EVERY_SITE: &str =
    "roots = [\".\", \"tools\"]\npr_only_checks = [\"DCO\"]\n[[binaries]]\npackage = \"app\"\n";
const RELEASE_KEY: &str = "RELEASE_APP_PRIVATE_KEY";
const CI_KEY: &str = "DEPS_APP_PRIVATE_KEY";
const API_ONLY: [&str; 5] = [
    "app-token",
    "revoke-token",
    "private-token-allowed",
    "release-gate",
    "release",
];

type Jobs = BTreeMap<String, Value>;

fn jobs_of(text: &str) -> Result<Jobs, Box<dyn Error>> {
    let document = crate::yaml_tree::document(text)?;
    Ok(document["jobs"]
        .as_object()
        .ok_or("no jobs")?
        .iter()
        .map(|(id, job)| (id.clone(), job.clone()))
        .collect())
}

fn release_workflows(helper: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut view = release_view(&[APP, TOOLS], Some(EVERY_SITE))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.81", helper).into_bytes());
    let every_site = expected_release(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    Ok(vec![every_site, generate(helper, TOOLS_LIKE)?.0])
}

fn ci_workflow(helper: &str) -> Result<String, Box<dyn Error>> {
    let repo = private_repo(&["Cargo.toml", "tools/Cargo.toml"], SECTION, helper)?;
    workflow(&repo)
}

fn steps(job: &Value) -> &[Value] {
    job["steps"].as_array().map_or(&[], Vec::as_slice)
}

fn step_run(step: &Value) -> &str {
    step["run"].as_str().unwrap_or_default()
}

fn runs_repository_code(step: &Value) -> bool {
    if let Some(uses) = step["uses"].as_str() {
        return !(uses.starts_with("tailrocks/velnor-new/actions/setup-velnor@")
            || uses.starts_with("actions/upload-artifact@")
            || uses.starts_with("actions/download-artifact@"));
    }
    let run = step_run(step);
    let command = run
        .strip_prefix("velnor-ci ")
        .or_else(|| run.strip_prefix(". \"$VELNOR_EXEC\" "))
        .and_then(|rest| rest.split_whitespace().next());
    !(run.starts_with("eval \"$VELNOR_HELPER_INSTALL_")
        || command.is_some_and(|name| API_ONLY.contains(&name)))
}

fn references(job: &Value, key: &str) -> bool {
    job.to_string().contains(key)
}

fn assert_key_jobs_run_no_repository_code(jobs: &Jobs, key: &str) {
    let holders: Vec<&String> = jobs
        .iter()
        .filter(|(_, job)| references(job, key))
        .map(|(id, _)| id)
        .collect();
    assert!(!holders.is_empty(), "no job references {key}");
    for id in holders {
        let code: Vec<&Value> = steps(&jobs[id])
            .iter()
            .filter(|step| runs_repository_code(step))
            .collect();
        assert!(code.is_empty(), "{id} references {key} and runs {code:?}");
    }
}

fn identity(step: &Value) -> &str {
    step["env"]["VELNOR_APP_JOB"].as_str().unwrap_or_default()
}

fn step_with<'a>(job: &'a Value, id: &str) -> Result<&'a Value, Box<dyn Error>> {
    steps(job)
        .iter()
        .find(|step| step["id"] == id)
        .ok_or_else(|| format!("no step {id}").into())
}

#[test]
fn from_helper_0_1_117_no_release_job_that_references_the_app_key_runs_repository_code()
-> TestResult {
    for text in release_workflows("0.1.117")? {
        let jobs = jobs_of(&text)?;
        assert_key_jobs_run_no_repository_code(&jobs, RELEASE_KEY);
        for building in ["release-pr", "tag"] {
            let job = &jobs[building];
            assert!(!references(job, RELEASE_KEY), "{building} holds the key");
            let request = step_with(job, "app-request")?;
            let received = step_with(job, "app")?;
            let relay = &jobs[&format!("app-token-{building}")];
            let relayed = step_with(relay, "app-relay")?;
            assert_eq!(identity(request), building);
            assert_eq!(identity(received), building);
            assert_eq!(identity(relayed), building);
            assert!(step_run(request).ends_with("app-token request --job=\"$VELNOR_APP_JOB\""));
            assert!(step_run(received).contains("app-token await "));
            assert!(
                step_run(relayed).contains("app-token relay --job=\"$VELNOR_APP_JOB\" --owner=")
            );
            assert_eq!(relay["needs"], job["needs"]);
            assert_eq!(relay["if"], job["if"]);
            assert_eq!(relay["environment"], job["environment"]);
            assert_eq!(relay["runs-on"], "ubuntu-latest");
            assert_eq!(job["permissions"]["actions"], "read");
            assert_eq!(relay["permissions"]["actions"], "read");
            let upload = steps(relay).last().ok_or("no steps")?;
            assert_eq!(
                upload["with"]["name"],
                "${{ steps.app-relay.outputs.artifact }}"
            );
            assert!(
                steps(job).iter().position(|s| s["id"] == "app-request")
                    < steps(job).iter().position(|s| s["name"] == "Check out")
            );
        }
    }
    Ok(())
}

#[test]
fn from_helper_0_1_117_no_ci_job_that_references_the_private_dependency_key_runs_repository_code()
-> TestResult {
    let jobs = jobs_of(&ci_workflow("0.1.117")?)?;
    assert_key_jobs_run_no_repository_code(&jobs, CI_KEY);
    let building: Vec<&String> = jobs
        .iter()
        .filter(|(_, job)| steps(job).iter().any(|s| s["id"] == "private-token"))
        .map(|(id, _)| id)
        .collect();
    assert!(!building.is_empty());
    for id in building {
        let job = &jobs[id];
        let relay = &jobs[&format!("app-token-{id}")];
        let token = step_with(job, "private-token")?;
        assert!(step_run(token).contains("app-token await "), "{token}");
        let guard = step_with(relay, "private-token-relay")?["if"]
            .as_str()
            .and_then(|text| text.strip_prefix("${{ "))
            .and_then(|text| text.strip_suffix(" }}"))
            .ok_or("the relay step has no guard")?;
        assert!(
            token["if"]
                .as_str()
                .is_some_and(|text| text.contains(guard))
        );
        assert!(step_with(relay, "private-check").is_ok());
        assert_eq!(relay["permissions"]["actions"], "read");
        assert_eq!(job["permissions"]["actions"], "read");
        assert_eq!(job["permissions"]["contents"], "read");
    }
    Ok(())
}

#[test]
fn older_pins_keep_the_key_in_the_job_that_runs_repository_code() -> TestResult {
    for text in release_workflows("0.1.116")? {
        let jobs = jobs_of(&text)?;
        assert!(jobs.keys().all(|id| !id.starts_with("app-token-")));
        assert!(references(&jobs["release-pr"], RELEASE_KEY));
    }
    let ci = jobs_of(&ci_workflow("0.1.116")?)?;
    assert!(ci.keys().all(|id| !id.starts_with("app-token-")));
    Ok(())
}
