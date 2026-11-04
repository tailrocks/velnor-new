// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{WitnessRead, WitnessRequest};
use crate::{OrchestratorError, crate_release_github::GitHubAdmissionRead, internal::internal};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    io::{Cursor, Write},
};

pub(super) struct Fixture {
    pub request: WitnessRequest,
    pub run: RefCell<Value>,
    pub job: RefCell<Value>,
    pub artifact: RefCell<Value>,
    pub log: Vec<u8>,
    pub bytes: Vec<u8>,
    pub change_artifact_after_download: Cell<bool>,
}

impl Fixture {
    pub(super) fn new() -> Result<Self, Box<dyn std::error::Error>> {
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
        let observation = json!({"schema":1,"observation":"script-exited-successfully","repository":request.repository,"workflow_sha":request.workflow_sha,"run_id":"33","run_attempt":"1","script":request.script,"source_sha256":request.source_sha256,"environment":request.environment,"reviewer_id":"55","unverified_candidate_pr":"66","unverified_candidate_sha":request.candidate_sha});
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive.start_file(
            "rehearsal-observation.json",
            zip::write::SimpleFileOptions::default(),
        )?;
        archive.write_all(serde_json::to_string(&observation)?.as_bytes())?;
        let bytes = archive.finish()?.into_inner();
        let digest = crate::generator::sha256_hex(&bytes);
        let run = json!({"id":33,"run_attempt":1,"head_sha":request.workflow_sha,"repository":{"id":12},"head_repository":{"id":12},"path":request.workflow_path,"event":"workflow_dispatch","status":"completed","conclusion":"success"});
        let job = json!({"id":44,"run_id":33,"run_attempt":1,"head_sha":request.workflow_sha,"name":"witness","status":"completed","conclusion":"success","steps":[{"name":"Bind observation artifact","status":"completed","conclusion":"success"}]});
        let artifact = json!({"id":77,"name":"rehearsal-observation","expired":false,"size_in_bytes":bytes.len(),"digest":format!("sha256:{digest}"),"workflow_run":{"id":33,"repository_id":12,"head_repository_id":12,"head_sha":request.workflow_sha}});
        let log = format!("2026-11-05T00:00:00.000Z VELNOR_REHEARSAL_WITNESS run=33 attempt=1 artifact=77 sha256={digest}\n").into_bytes();
        Ok(Self {
            request,
            run: RefCell::new(run),
            job: RefCell::new(job),
            artifact: RefCell::new(artifact),
            log,
            bytes,
            change_artifact_after_download: Cell::new(false),
        })
    }
}

impl GitHubAdmissionRead for Fixture {
    fn get(&self, path: &str) -> Result<Value, OrchestratorError> {
        if path == "repos/acme/example/actions/runs/33/attempts/1" {
            Ok(self.run.borrow().clone())
        } else if path == "repos/acme/example/actions/jobs/44" {
            Ok(self.job.borrow().clone())
        } else if path == "repos/acme/example/actions/artifacts/77" {
            Ok(self.artifact.borrow().clone())
        } else {
            Err(internal("unexpected_external_request"))
        }
    }
    fn get_optional(&self, path: &str) -> Result<Option<Value>, OrchestratorError> {
        self.get(path).map(Some)
    }
}
impl WitnessRead for Fixture {
    fn witness_job_log(
        &self,
        _repository: &str,
        _job_id: u64,
    ) -> Result<Vec<u8>, OrchestratorError> {
        Ok(self.log.clone())
    }
    fn artifact_bytes(
        &self,
        _repository: &str,
        _artifact_id: u64,
        _expected_size: u64,
        _expected_sha256: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        if self.change_artifact_after_download.get() {
            self.artifact.borrow_mut()["expired"] = true.into();
        }
        Ok(self.bytes.clone())
    }
}
