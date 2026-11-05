// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::crate_release_github::GitHubAdmissionRead;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};

struct Fixture {
    request: WitnessRequest,
    run: Value,
    job: RefCell<Value>,
    log: RefCell<Vec<u8>>,
    mutate: Cell<bool>,
}
impl Fixture {
    fn new() -> Self {
        let request = WitnessRequest {
            repository: "acme/example".into(),
            repository_id: 12,
            workflow_sha: "a".repeat(40),
            workflow_path: ".github/workflows/release-rehearsal.yml".into(),
            run_id: 33,
            run_attempt: 1,
            job_id: 44,
            environment: "staging".into(),
            reviewer_id: 55,
            script: "verify/run.sh".into(),
            source_sha256: "b".repeat(64),
            candidate_pr: 66,
            candidate_sha: "c".repeat(40),
        };
        let run = json!({"id":33,"run_attempt":1,"head_sha":request.workflow_sha,"repository":{"id":12},"head_repository":{"id":12},"path":request.workflow_path,"event":"workflow_dispatch","status":"completed","conclusion":"success"});
        let job = json!({"id":45,"run_id":33,"run_attempt":1,"head_sha":request.workflow_sha,"name":"execute","status":"completed","conclusion":"success","steps":[{"name":"Run rehearsal without credentials","status":"completed","conclusion":"success"}]});
        Self {
            request,
            run,
            job: RefCell::new(job),
            log: RefCell::new(
                b"2026-11-05T00:00:00Z FIXTURE_OBSERVATION={\"schema\":1}\n".to_vec(),
            ),
            mutate: Cell::new(false),
        }
    }
    fn verify(&self) -> Result<(), OrchestratorError> {
        verify_executor_observation(
            &self.request,
            45,
            "FIXTURE_OBSERVATION=",
            br#"{"schema":1}"#,
            self,
        )
    }
}
impl GitHubAdmissionRead for Fixture {
    fn get(&self, path: &str) -> Result<Value, OrchestratorError> {
        if path.ends_with("/actions/runs/33/attempts/1") {
            Ok(self.run.clone())
        } else if path.ends_with("/actions/jobs/45") {
            Ok(self.job.borrow().clone())
        } else {
            Err(invalid())
        }
    }
    fn get_optional(&self, path: &str) -> Result<Option<Value>, OrchestratorError> {
        self.get(path).map(Some)
    }
}
impl WitnessRead for Fixture {
    fn witness_job_log(&self, _: &str, _: u64) -> Result<Vec<u8>, OrchestratorError> {
        if self.mutate.get() {
            self.job.borrow_mut()["conclusion"] = "failure".into();
        }
        Ok(self.log.borrow().clone())
    }
    fn artifact_bytes(
        &self,
        _: &str,
        _: u64,
        _: u64,
        _: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        Err(invalid())
    }
}
#[test]
fn accepts_only_exact_unique_authenticated_observation() {
    let f = Fixture::new();
    assert!(f.verify().is_ok());
    for log in [
        b"2026-11-05T00:00:00Z FIXTURE_OBSERVATION={\"schema\":2}\n".as_slice(),
        b"2026-11-05T00:00:00Z FIXTURE_OBSERVATION={\"schema\":1,\"schema\":1}\n",
        b"2026-11-05T00:00:00Z FIXTURE_OBSERVATION={\"schema\":1}",
        b"2026-11-05T00:00:00Z FIXTURE_OBSERVATION={\"schema\":1}\n\xff\n",
        b"FIXTURE_OBSERVATION={\"schema\":1}\n",
        b"2026-11-05T00:00:00Z FIXTURE_OBSERVATION={\"schema\":1}\n2026-11-05T00:00:00Z FIXTURE_OBSERVATION={\"schema\":1}\n",
    ] { *f.log.borrow_mut() = log.to_vec(); assert!(f.verify().is_err()); }
    *f.log.borrow_mut() = format!(
        "2026-11-05T00:00:00Z FIXTURE_OBSERVATION={{\"x\":\"{}\"}}\n",
        "x".repeat(MAX_RECORD)
    )
    .into_bytes();
    assert!(f.verify().is_err());
}
#[test]
fn rejects_wrong_or_changing_job_identity() {
    let f = Fixture::new();
    f.job.borrow_mut()["run_id"] = 34.into();
    assert!(f.verify().is_err());
    f.job.borrow_mut()["run_id"] = 33.into();
    f.job.borrow_mut()["run_attempt"] = 2.into();
    assert!(f.verify().is_err());
    f.job.borrow_mut()["run_attempt"] = 1.into();
    f.mutate.set(true);
    assert!(f.verify().is_err());
}

#[test]
fn rejects_invalid_marker_and_expected_input() {
    let f = Fixture::new();
    for marker in ["", "A\n=", "../=", "A", "="] {
        assert!(
            verify_executor_observation(&f.request, 45, marker, br#"{"schema":1}"#, &f).is_err()
        );
    }
    assert!(
        verify_executor_observation(
            &f.request,
            45,
            "FIXTURE_OBSERVATION=",
            &vec![b' '; MAX_RECORD + 1],
            &f
        )
        .is_err()
    );
    assert!(
        verify_executor_observation(
            &f.request,
            45,
            "FIXTURE_OBSERVATION=",
            br#"{"x":1,"x":1}"#,
            &f
        )
        .is_err()
    );
}
