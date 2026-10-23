use std::time::Instant;

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_contract::{
    GITHUB_EVIDENCE_SNAPSHOT_SCHEMA, GitHubEvidenceArtifactRefV1, GitHubEvidenceFetchSpecV1,
    GitHubEvidenceSnapshotV1, GitHubEvidenceWorkflowRefV1, GitHubEvidenceWorkflowSourceRefV1,
    mise_task_suite_artifact_name_for_attempt,
};

#[path = "github_evidence_collect_test_scenario.rs"]
mod scenario;
pub(super) use scenario::{FixtureReader, FixtureScenario};

use super::super::*;
use super::{PREDECESSOR_TIME, RUN_TIME, run_identity, run_summary};
use crate::api::github_evidence_http::{
    ArtifactArchiveRequest, DownloadReceipt, EvidenceRoute, GitHubEvidenceRead,
};

impl FixtureReader {
    pub(super) const fn for_scenario(scenario: FixtureScenario) -> Self {
        Self {
            scenario,
            omit_attempt_artifact: None,
        }
    }

    pub(super) const fn omitting_attempt_artifact(attempt: u32) -> Self {
        Self {
            scenario: FixtureScenario::Complete,
            omit_attempt_artifact: Some(attempt),
        }
    }

    fn is_scenario(&self, scenario: FixtureScenario) -> bool {
        self.scenario == scenario
    }

    fn has_paged_jobs(&self) -> bool {
        matches!(
            self.scenario,
            FixtureScenario::MultiPageJobs | FixtureScenario::TruncatedJobsSecondPage
        )
    }

    fn response(&self, route: &EvidenceRoute) -> Result<Value, OrchestratorError> {
        match route {
            EvidenceRoute::Repository => Ok(json!({
                "id": 1,
                "name": "velnor",
                "full_name": "tailrocks/velnor",
                "default_branch": "main",
                "owner": { "login": "tailrocks" }
            })),
            EvidenceRoute::Workflows { .. } => Ok(self.workflow_inventory()),
            EvidenceRoute::WorkflowRuns { workflow_id, .. } => self.workflow_runs(*workflow_id),
            EvidenceRoute::WorkflowPredecessor { workflow_id, .. } => {
                Ok(self.predecessor(*workflow_id))
            }
            EvidenceRoute::RunAttempt { run_id, attempt } => {
                Ok(self.run_attempt(*run_id, *attempt))
            }
            EvidenceRoute::Jobs {
                run_id,
                attempt,
                page,
            } => Ok(self.jobs(*run_id, *attempt, *page)),
            EvidenceRoute::RunArtifacts { run_id, .. } => Ok(self.run_artifacts(*run_id)),
            EvidenceRoute::ArtifactById { artifact_id } => {
                let attempt = u32::try_from(artifact_id.saturating_sub(500)).unwrap_or_default();
                Ok(self.artifact_json(*artifact_id, attempt))
            }
            EvidenceRoute::ArtifactDownload { .. } => Err(crate::internal::internal(
                "fixture_unexpected_download_route",
            )),
        }
    }

    fn workflow_inventory(&self) -> Value {
        let workflows = vec![
            json!({
                "id": 10,
                "path": ".github/workflows/ci.yml",
                "state": "active"
            }),
            json!({
                "id": 20,
                "path": ".github/workflows/producer.yml",
                "state": "active"
            }),
        ];
        json!({
            "total_count": 2,
            "workflows": if self.is_scenario(FixtureScenario::PartialWorkflowPage) {
                workflows[..1].to_vec()
            } else {
                workflows
            }
        })
    }

    fn workflow_runs(&self, workflow_id: u64) -> Result<Value, OrchestratorError> {
        let (id, attempts, conclusion) = match workflow_id {
            10 => (100, 2, "success"),
            20 => (200, 2, "success"),
            _ => return Err(crate::internal::internal("fixture_workflow")),
        };
        let rows = if workflow_id == 20 && self.is_scenario(FixtureScenario::OmitProducerRun) {
            Vec::new()
        } else {
            vec![run_summary(id, workflow_id, attempts, RUN_TIME, conclusion)]
        };
        Ok(json!({ "total_count": rows.len(), "workflow_runs": rows }))
    }

    fn predecessor(&self, workflow_id: u64) -> Value {
        if self.is_scenario(FixtureScenario::OmitPredecessor) {
            return json!({ "total_count": 0, "workflow_runs": [] });
        }
        let id = if workflow_id == 10 { 101 } else { 201 };
        json!({
            "total_count": 1,
            "workflow_runs": [run_summary(
                id,
                workflow_id,
                1,
                PREDECESSOR_TIME,
                "success"
            )]
        })
    }

    fn run_attempt(&self, run_id: u64, attempt: u32) -> Value {
        let (workflow_id, created_at) = run_identity(run_id);
        let conclusion = if run_id == 100 && attempt == 1 {
            "startup_failure"
        } else if run_id == 200 && attempt == 1 {
            "failure"
        } else {
            "success"
        };
        json!({
            "id": run_id,
            "workflow_id": workflow_id,
            "run_attempt": attempt,
            "event": "push",
            "head_branch": "main",
            "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "status": "completed",
            "conclusion": if self.is_scenario(FixtureScenario::BadConclusion) {
                "unknown"
            } else {
                conclusion
            },
            "created_at": created_at
        })
    }

    fn jobs(&self, run_id: u64, attempt: u32, page: u32) -> Value {
        let task_job_name = if run_id == 200 || run_id == 201 {
            "Mise task producer"
        } else {
            "CI build"
        };
        if self.has_paged_jobs() {
            return self.paged_jobs(run_id, attempt, page, task_job_name);
        }
        let mut response = json!({
            "total_count": if self.is_scenario(FixtureScenario::DuplicateJobId) {
                2
            } else {
                1
            },
            "jobs": if self.is_scenario(FixtureScenario::DuplicateJobId) {
                vec![
                    json!({
                        "id": run_id * 10 + u64::from(attempt),
                        "run_id": run_id,
                        "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "name": task_job_name
                    }),
                    json!({
                        "id": run_id * 10 + u64::from(attempt),
                        "run_id": run_id,
                        "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "name": task_job_name
                    })
                ]
            } else {
                vec![json!({
                "id": run_id * 10 + u64::from(attempt),
                "run_id": run_id,
                "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "name": if self.is_scenario(FixtureScenario::WrongTaskJobName) {
                    "Wrong task job"
                } else {
                    task_job_name
                }
            })]
            }
        });
        if self.is_scenario(FixtureScenario::WrongJobRunId) {
            response["jobs"][0]["run_id"] = json!(run_id + 1);
        }
        if self.is_scenario(FixtureScenario::WrongJobHeadSha) {
            response["jobs"][0]["head_sha"] = json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        }
        if self.is_scenario(FixtureScenario::MissingJobRunId) {
            response["jobs"][0]
                .as_object_mut()
                .expect("job object")
                .remove("run_id");
        }
        if self.is_scenario(FixtureScenario::MissingJobHeadSha) {
            response["jobs"][0]
                .as_object_mut()
                .expect("job object")
                .remove("head_sha");
        }
        response
    }

    fn paged_jobs(&self, run_id: u64, attempt: u32, page: u32, task_job_name: &str) -> Value {
        let jobs = if page == 1 {
            (0..100)
                .map(|index| {
                    json!({
                        "id": run_id * 1_000 + u64::from(attempt) * 100 + index,
                        "run_id": run_id,
                        "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "name": format!("Other job {index}")
                    })
                })
                .collect::<Vec<_>>()
        } else if self.is_scenario(FixtureScenario::TruncatedJobsSecondPage) {
            Vec::new()
        } else {
            vec![json!({
                "id": run_id * 1_000 + u64::from(attempt) * 100 + 100,
                "run_id": run_id,
                "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "name": task_job_name
            })]
        };
        json!({ "total_count": 101, "jobs": jobs })
    }

    fn run_artifacts(&self, run_id: u64) -> Value {
        let rows = if run_id == 200 {
            let mut rows = (1..=2)
                .filter(|attempt| Some(*attempt) != self.omit_attempt_artifact)
                .map(|attempt| self.artifact_json(500 + u64::from(attempt), attempt))
                .collect::<Vec<_>>();
            if self.is_scenario(FixtureScenario::ExtraAttemptArtifact) {
                rows.push(self.artifact_json(503, 3));
            }
            rows
        } else {
            Vec::new()
        };
        json!({ "total_count": rows.len(), "artifacts": rows })
    }

    fn archive_fixture(&self) -> Vec<u8> {
        if self.is_scenario(FixtureScenario::InvalidArchive) {
            b"PK\x03\x04".to_vec()
        } else {
            crate::api::github_evidence::test_support::archive()
        }
    }

    fn artifact_json(&self, id: u64, attempt: u32) -> Value {
        let archive = self.archive_fixture();
        json!({
            "id": id,
            "name": mise_task_suite_artifact_name_for_attempt("producer", "ledger", 200, attempt),
            "size_in_bytes": archive.len(),
            "expired": false,
            "digest": format!("sha256:{}", bundle::sha256_hex(&archive)),
            "workflow_run": {
                "id": 200,
                "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "head_branch": "main",
                "repository_id": 1,
                "head_repository_id": 1
            }
        })
    }
}

impl GitHubEvidenceRead for FixtureReader {
    fn json(
        &self,
        _repo: &GitHubRepository,
        route: &EvidenceRoute,
        _token: &str,
        _deadline: Instant,
    ) -> Result<Vec<u8>, OrchestratorError> {
        let body = self.response(route)?;
        serde_json::to_vec(&body).map_err(|_| crate::internal::internal("fixture_json_encode"))
    }

    fn archive(
        &self,
        request: ArtifactArchiveRequest<'_>,
    ) -> Result<DownloadReceipt, OrchestratorError> {
        let archive = self.archive_fixture();
        let digest = bundle::sha256_hex(&archive);
        if !matches!(request.artifact_id, 501 | 502)
            || request.expected_size != archive.len() as u64
            || request.expected_sha256 != digest
        {
            return Err(crate::internal::internal("fixture_archive_identity"));
        }
        std::fs::write(request.dest, &archive)
            .map_err(|_| crate::internal::internal("fixture_archive_write"))?;
        Ok(DownloadReceipt {
            size_bytes: archive.len() as u64,
            sha256: digest,
        })
    }
}

pub(super) fn spec() -> GitHubEvidenceFetchSpecV1 {
    GitHubEvidenceFetchSpecV1 {
        schema: GITHUB_EVIDENCE_SNAPSHOT_SCHEMA,
        collector_suite_id: "collector".to_owned(),
        collector_workflow_path: ".github/workflows/nightly.yml".to_owned(),
        default_branch: "main".to_owned(),
        window_days: 31,
        workflow_sources: vec![
            GitHubEvidenceWorkflowRefV1 {
                source: GitHubEvidenceWorkflowSourceRefV1::PrimaryCi,
                path: ".github/workflows/ci.yml".to_owned(),
                job_id: None,
                job_name: None,
            },
            GitHubEvidenceWorkflowRefV1 {
                source: GitHubEvidenceWorkflowSourceRefV1::TaskSuite {
                    suite_id: "producer".to_owned(),
                },
                path: ".github/workflows/producer.yml".to_owned(),
                job_id: Some("mise-task-producer".to_owned()),
                job_name: Some("Mise task producer".to_owned()),
            },
        ],
        artifact_sources: vec![GitHubEvidenceArtifactRefV1 {
            suite_id: "producer".to_owned(),
            artifact_name: "ledger".to_owned(),
            workflow_path: ".github/workflows/producer.yml".to_owned(),
            job_id: "mise-task-producer".to_owned(),
            job_name: "Mise task producer".to_owned(),
        }],
    }
}

fn invocation() -> Invocation {
    Invocation {
        spec: spec(),
        token: "fixture-token".to_owned(),
        repository: GitHubRepository {
            owner: "tailrocks".to_owned(),
            name: "velnor".to_owned(),
        },
        repository_id: 1,
        run_id: 900,
        run_attempt: 1,
        ref_name: "refs/heads/main".to_owned(),
        sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
        window: time::EvidenceWindow {
            captured_at: 1_792_713_600,
            start: 1_790_035_200,
            end: 1_792_713_600,
        },
    }
}

pub(super) fn collect<R: GitHubEvidenceRead>(
    reader: &R,
    temp: &TempDir,
) -> Result<GitHubEvidenceSnapshotV1, OrchestratorError> {
    let bundle = bundle::BundleRoot::create_at(temp.path())?;
    let mut collector = Collector::new(reader, invocation(), bundle);
    collector.capture_repository()?;
    let workflow_ids = collector.capture_workflow_inventory()?;
    let attempts = collector.capture_workflows(&workflow_ids)?;
    collector.capture_artifacts(&attempts)?;
    collector.finish()?;
    let index = std::fs::read(temp.path().join("velnor/github-evidence-bundle/index.json"))
        .map_err(|_| crate::internal::internal("fixture_index_read"))?;
    serde_json::from_slice(&index).map_err(|_| crate::internal::internal("fixture_index_parse"))
}
