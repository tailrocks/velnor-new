//! Sanitized GitHub run and pull request API records.

use velnor_runner_github::workflow_run::RepositoryTarget;
use velnor_runner_github::{CompleteSourceMetadata, SourceMetadata};

pub(super) const OWNER: &str = "ChainArgos";
pub(super) const REPOSITORY: &str = "java-monorepo";
pub(super) const REPOSITORY_ID: i64 = 41;
pub(super) const HEAD_REPOSITORY_ID: i64 = 73;
pub(super) const RUN_ID: i64 = 2468;
pub(super) const HEAD_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const BASE_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const MERGE_SHA: &str = "cccccccccccccccccccccccccccccccccccccccc";
pub(super) const WORKFLOW_REF: &str =
    "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/sea/integration";

pub(super) fn target() -> RepositoryTarget<'static> {
    RepositoryTarget {
        id: REPOSITORY_ID,
        owner: OWNER,
        name: REPOSITORY,
    }
}

pub(super) fn repository_json(id: i64, owner: &str, name: &str, private: bool) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","full_name":"{owner}/{name}","private":{private},"owner":{{"login":"{owner}"}}}}"#
    )
}

pub(super) fn source_metadata() -> SourceMetadata {
    SourceMetadata {
        owner_name: Some(OWNER.to_owned()),
        repository_name: Some(REPOSITORY.to_owned()),
        job_workflow_ref: Some(WORKFLOW_REF.to_owned()),
        workflow_run_id: Some(RUN_ID),
        event_name: Some("push".to_owned()),
    }
}

pub(super) fn empty_source() -> CompleteSourceMetadata<'static> {
    CompleteSourceMetadata {
        owner_name: "",
        repository_name: "",
        job_workflow_ref: "",
        workflow_run_id: 0,
        event_name: "",
    }
}

pub(super) fn workflow_run_json(event: &str, branch: &str) -> String {
    format!(
        r#"{{"id":{RUN_ID},"event":"{event}","path":".github/workflows/ci.yml","head_branch":"{branch}","head_sha":"{HEAD_SHA}","workflow_id":17,"run_attempt":1,"repository":{{"id":{REPOSITORY_ID},"name":"{REPOSITORY}","full_name":"{OWNER}/{REPOSITORY}","owner":{{"login":"{OWNER}"}}}},"head_repository":{{"id":{REPOSITORY_ID},"name":"{REPOSITORY}","full_name":"{OWNER}/{REPOSITORY}","owner":{{"login":"{OWNER}"}}}},"pull_requests":[],"referenced_workflows":[{{"path":"{WORKFLOW_REF}","sha":"{HEAD_SHA}","ref":"refs/heads/sea/integration"}}]}}"#
    )
}

pub(super) fn pull_request_run_json() -> String {
    format!(
        r#"{{"id":{RUN_ID},"event":"pull_request","path":".github/workflows/ci.yml","head_branch":"feature","head_sha":"{HEAD_SHA}","workflow_id":17,"run_attempt":1,"repository":{{"id":{REPOSITORY_ID},"name":"{REPOSITORY}","full_name":"{OWNER}/{REPOSITORY}","owner":{{"login":"{OWNER}"}}}},"head_repository":{{"id":{HEAD_REPOSITORY_ID},"name":"fork-repo","full_name":"fork-owner/fork-repo","owner":{{"login":"fork-owner"}}}},"pull_requests":[{{"number":2110,"head":{{"ref":"feature","sha":"{HEAD_SHA}","repo":{{"id":{HEAD_REPOSITORY_ID},"url":"https://api.github.com/repos/fork-owner/fork-repo","name":"fork-repo"}}}},"base":{{"ref":"sea/integration","sha":"{BASE_SHA}","repo":{{"id":{REPOSITORY_ID},"url":"https://api.github.com/repos/{OWNER}/{REPOSITORY}","name":"{REPOSITORY}"}}}}}}]}}"#
    )
}

pub(super) fn pull_request_json() -> String {
    format!(
        r#"{{"number":2110,"state":"open","merged":false,"head":{{"ref":"feature","sha":"{HEAD_SHA}","repo":{{"id":{HEAD_REPOSITORY_ID},"name":"fork-repo","full_name":"fork-owner/fork-repo","owner":{{"login":"fork-owner"}}}}}},"base":{{"ref":"sea/integration","sha":"{BASE_SHA}","repo":{{"id":{REPOSITORY_ID},"name":"{REPOSITORY}","full_name":"{OWNER}/{REPOSITORY}","owner":{{"login":"{OWNER}"}}}}}},"merge_commit_sha":"{MERGE_SHA}"}}"#
    )
}
