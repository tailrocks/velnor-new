// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use velnor_actions_orchestrator::crate_release_github::authorize_github_candidate;

struct Origin {
    pointer: &'static str,
    value: Value,
}

impl GitHubAdmissionRead for Origin {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let mut response = Fixture::valid().get(route)?;
        if route.contains("/actions/runs?") {
            response["workflow_runs"][0][self.pointer] = self.value.clone();
        }
        Ok(response)
    }
    fn get_optional(&self, _: &str) -> Result<Option<Value>, OrchestratorError> {
        Ok(None)
    }
}

#[test]
fn release_required_ci_requires_default_branch_push_and_repository_identity() {
    assert!(verify_github_evidence(&request(), &Fixture::valid()).is_ok());
    for (pointer, value) in [
        ("event", json!("pull_request")),
        ("event", json!("workflow_dispatch")),
        ("head_branch", json!("auxiliary")),
        ("event", Value::Null),
        (
            "repository",
            json!({"id":999,"full_name":"example/crate","owner":{"id":202,"login":"example"}}),
        ),
        (
            "head_repository",
            json!({"id":999,"full_name":"example/crate","owner":{"id":202,"login":"example"}}),
        ),
        (
            "repository",
            json!({"id":101,"full_name":"foreign/crate","owner":{"id":202,"login":"example"}}),
        ),
        (
            "head_repository",
            json!({"id":101,"full_name":"example/crate","owner":{"id":999,"login":"example"}}),
        ),
        ("head_repository", Value::Null),
    ] {
        assert!(
            verify_github_evidence(&request(), &Origin { pointer, value }).is_err(),
            "accepted {pointer}"
        );
    }
}

struct Rerun {
    same_suite: bool,
    equal_started: bool,
    latest_app_id: u64,
    conclusion: &'static str,
    status: &'static str,
    recorded: bool,
    recorded_conclusion: &'static str,
    recorded_event: &'static str,
}

impl GitHubAdmissionRead for Rerun {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        if route.contains("/attempts/") && route.contains("/jobs?") {
            let (run_id, attempt) = if self.same_suite { (501, 2) } else { (502, 1) };
            if route.contains(&format!("/runs/{run_id}/attempts/{attempt}/jobs?")) {
                return Ok(
                    json!({"total_count":1,"jobs":[{"id":2,"name":"Required","run_id":run_id,"run_attempt":attempt,"check_run_url":"https://api.github.com/repos/example/crate/check-runs/2","status":self.status,"conclusion":self.conclusion}]}),
                );
            }
        }
        let mut response = Fixture::valid().get(route)?;
        if route.contains("check-runs?") {
            let mut old = response["check_runs"][0].clone();
            old["conclusion"] = json!(self.recorded_conclusion);
            let mut new = response["check_runs"][0].clone();
            new["id"] = json!(2);
            new["check_suite"]["id"] = json!(if self.same_suite { 123 } else { 124 });
            if !self.equal_started {
                new["started_at"] = json!("2026-11-03T15:01:00Z");
            }
            new["app"]["id"] = json!(self.latest_app_id);
            new["status"] = json!(self.status);
            new["conclusion"] = json!(self.conclusion);
            let checks = if self.recorded {
                vec![old, new]
            } else {
                vec![new]
            };
            response = json!({"total_count":checks.len(),"check_runs":checks});
        }
        if route.contains("/actions/runs?") {
            let mut old = response["workflow_runs"][0].clone();
            let mut new = old.clone();
            new["check_suite_id"] = json!(124);
            new["id"] = json!(502);
            new["run_number"] = json!(2);
            old["event"] = json!(self.recorded_event);
            let runs = if self.same_suite {
                old["run_attempt"] = json!(2);
                vec![old]
            } else {
                vec![old, new]
            };
            response = json!({"total_count":runs.len(),"workflow_runs":runs});
        }
        Ok(response)
    }
    fn get_optional(&self, _: &str) -> Result<Option<Value>, OrchestratorError> {
        Ok(None)
    }
}

#[test]
fn successful_ci_rerun_preserves_admitted_check_and_rejects_lost_or_failed_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    for same_suite in [true, false] {
        rerun_lifecycle(same_suite)?;
    }
    Ok(())
}

fn rerun_lifecycle(same_suite: bool) -> Result<(), Box<dyn std::error::Error>> {
    let input = request();
    let admitted = verify_github_evidence(&input, &Fixture::valid())?;
    let mut rerun = Rerun {
        same_suite,
        equal_started: false,
        latest_app_id: 15368,
        conclusion: "success",
        status: "completed",
        recorded: true,
        recorded_conclusion: "success",
        recorded_event: "push",
    };
    let authorized = authorize_github_candidate(&input, &admitted, &rerun)?;
    assert_eq!(authorized.candidate(), &admitted);
    rerun.conclusion = "failure";
    assert!(authorize_github_candidate(&input, &admitted, &rerun).is_err());
    rerun.conclusion = "";
    rerun.status = "in_progress";
    assert!(authorize_github_candidate(&input, &admitted, &rerun).is_err());
    rerun.conclusion = "success";
    rerun.status = "completed";
    rerun.recorded = false;
    assert!(authorize_github_candidate(&input, &admitted, &rerun).is_err());
    rerun.recorded = true;
    rerun.recorded_conclusion = "failure";
    assert!(authorize_github_candidate(&input, &admitted, &rerun).is_err());
    rerun.recorded_conclusion = "success";
    rerun.recorded_event = "pull_request";
    assert!(authorize_github_candidate(&input, &admitted, &rerun).is_err());
    rerun.recorded_event = "push";
    let mut forged = admitted.clone();
    forged.release_pr = Some(999);
    assert!(authorize_github_candidate(&input, &forged, &rerun).is_err());
    Ok(())
}

#[test]
fn equal_start_times_select_authenticated_latest_required_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    for same_suite in [true, false] {
        let mut rerun = Rerun {
            same_suite,
            equal_started: true,
            latest_app_id: 15368,
            conclusion: "success",
            status: "completed",
            recorded: true,
            recorded_conclusion: "failure",
            recorded_event: "push",
        };
        let candidate = verify_github_evidence(&request(), &rerun)?;
        assert_eq!(candidate.required_check_id, 2);

        rerun.recorded_conclusion = "success";
        rerun.conclusion = "failure";
        assert!(matches!(
            verify_github_evidence(&request(), &rerun),
            Err(OrchestratorError::Internal { problem }) if problem == "crate_release_required_failed"
        ));

        rerun.status = "in_progress";
        rerun.conclusion = "";
        assert!(verify_github_evidence(&request(), &rerun).is_err());

        rerun.status = "completed";
        rerun.conclusion = "success";
        rerun.latest_app_id = 42;
        assert!(matches!(
            verify_github_evidence(&request(), &rerun),
            Err(OrchestratorError::Internal { problem }) if problem == "crate_release_check_app"
        ));
    }
    Ok(())
}
