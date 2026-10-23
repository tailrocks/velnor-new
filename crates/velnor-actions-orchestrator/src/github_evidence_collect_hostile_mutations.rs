use serde_json::{Value, json};

use super::{EvidenceRoute, Mutation};

pub(super) fn apply(mutation: Mutation, route: &EvidenceRoute, value: &mut Value) {
    mutate_repository_and_runs(mutation, route, value);
    mutate_attempts(mutation, route, value);
    mutate_artifacts(mutation, route, value);
}

fn mutate_repository_and_runs(mutation: Mutation, route: &EvidenceRoute, value: &mut Value) {
    match (mutation, route) {
        (Mutation::RepositoryId, EvidenceRoute::Repository) => value["id"] = json!(2),
        (Mutation::RepositoryName, EvidenceRoute::Repository) => {
            value["name"] = json!("fork");
            value["full_name"] = json!("tailrocks/fork");
        }
        (Mutation::WorkflowPath, EvidenceRoute::Workflows { page: 1 }) => {
            value["workflows"][0]["path"] = json!(".github/workflows/other.yml");
        }
        (
            Mutation::RunWorkflowId,
            EvidenceRoute::WorkflowRuns {
                workflow_id: 10, ..
            },
        ) => {
            value["workflow_runs"][0]["workflow_id"] = json!(11);
        }
        (
            Mutation::RunEvent,
            EvidenceRoute::WorkflowRuns {
                workflow_id: 10, ..
            },
        ) => {
            value["workflow_runs"][0]["event"] = json!("pull_request");
        }
        (
            Mutation::RunBranch,
            EvidenceRoute::WorkflowRuns {
                workflow_id: 10, ..
            },
        ) => {
            value["workflow_runs"][0]["head_branch"] = json!("release");
        }
        (
            Mutation::RunHead,
            EvidenceRoute::WorkflowRuns {
                workflow_id: 10, ..
            },
        ) => {
            value["workflow_runs"][0]["head_sha"] =
                json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        }
        (Mutation::DuplicateWorkflowId, EvidenceRoute::Workflows { page: 1 }) => {
            value["workflows"][1]["id"] = json!(10);
        }
        (
            Mutation::DuplicateRunId,
            EvidenceRoute::WorkflowRuns {
                workflow_id: 10,
                page: 1,
                ..
            },
        ) => {
            let run = value["workflow_runs"][0].clone();
            value["workflow_runs"] = json!([run.clone(), run]);
            value["total_count"] = json!(2);
        }
        _ => {}
    }
}

fn mutate_attempts(mutation: Mutation, route: &EvidenceRoute, value: &mut Value) {
    match (mutation, route) {
        (
            Mutation::AttemptRunId,
            EvidenceRoute::RunAttempt {
                run_id: 100,
                attempt: 1,
            },
        ) => {
            value["id"] = json!(101);
        }
        (
            Mutation::AttemptNumber,
            EvidenceRoute::RunAttempt {
                run_id: 100,
                attempt: 1,
            },
        ) => {
            value["run_attempt"] = json!(2);
        }
        (
            Mutation::AttemptEvent,
            EvidenceRoute::RunAttempt {
                run_id: 100,
                attempt: 1,
            },
        ) => {
            value["event"] = json!("pull_request");
        }
        (
            Mutation::AttemptBranch,
            EvidenceRoute::RunAttempt {
                run_id: 100,
                attempt: 1,
            },
        ) => {
            value["head_branch"] = json!("release");
        }
        (
            Mutation::AttemptHead,
            EvidenceRoute::RunAttempt {
                run_id: 100,
                attempt: 1,
            },
        ) => {
            value["head_sha"] = json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        }
        _ => {}
    }
}

fn mutate_artifacts(mutation: Mutation, route: &EvidenceRoute, value: &mut Value) {
    match (mutation, route) {
        (
            Mutation::DuplicateArtifactId,
            EvidenceRoute::RunArtifacts {
                run_id: 200,
                page: 1,
            },
        ) => {
            let first_id = value["artifacts"][0]["id"].clone();
            value["artifacts"][1]["id"] = first_id;
        }
        (
            Mutation::ArtifactExpired,
            EvidenceRoute::RunArtifacts {
                run_id: 200,
                page: 1,
            },
        ) => {
            value["artifacts"][0]["expired"] = json!(true);
        }
        (
            Mutation::ArtifactEmpty,
            EvidenceRoute::RunArtifacts {
                run_id: 200,
                page: 1,
            },
        ) => {
            value["artifacts"][0]["size_in_bytes"] = json!(0);
        }
        (
            Mutation::ArtifactWrongSize,
            EvidenceRoute::RunArtifacts {
                run_id: 200,
                page: 1,
            },
        ) => {
            let size = value["artifacts"][0]["size_in_bytes"]
                .as_u64()
                .unwrap_or_default();
            value["artifacts"][0]["size_in_bytes"] = json!(size + 1);
        }
        (
            Mutation::ArtifactWrongSize | Mutation::ArtifactByIdChanged,
            EvidenceRoute::ArtifactById { artifact_id: 501 },
        ) => {
            let size = value["size_in_bytes"].as_u64().unwrap_or_default();
            value["size_in_bytes"] = json!(size + 1);
        }
        (
            Mutation::ArtifactWrongDigest,
            EvidenceRoute::RunArtifacts {
                run_id: 200,
                page: 1,
            },
        ) => {
            value["artifacts"][0]["digest"] = json!(format!("sha256:{}", "d".repeat(64)));
        }
        (Mutation::ArtifactWrongDigest, EvidenceRoute::ArtifactById { artifact_id: 501 }) => {
            value["digest"] = json!(format!("sha256:{}", "d".repeat(64)));
        }
        _ => {}
    }
}
