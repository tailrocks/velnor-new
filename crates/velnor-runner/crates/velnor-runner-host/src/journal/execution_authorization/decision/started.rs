//! Exact request and native event linkage for the current Runner.

use crate::error::HostError;
use crate::launch::execution_authorization::AuthorizedExecutionSource;
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "started_tests.rs"]
mod tests;

pub(in crate::journal::execution_authorization) struct StartedExecution {
    pub(super) request_id: i64,
    pub(super) event_digest: String,
    pub(super) native_job_id: Option<String>,
    pub(super) runner_id: i64,
    pub(super) runner_name: String,
    pub(super) jit_runner_id: Option<i64>,
    pub(super) jit_runner_name: Option<String>,
    pub(super) jit_started_request_id: Option<i64>,
    pub(super) jit_state: String,
    pub(super) jit_completed_request_id: Option<i64>,
    pub(super) jit_witness_request_id: i64,
    pub(super) acquire_state: String,
    pub(super) completed_event_digest: Option<String>,
    pub(super) state_delivery_state: String,
    pub(super) state_resources_bound: bool,
    pub(super) state_instance_binding_digest: Option<String>,
    pub(super) reservation_state: String,
    pub(super) runner_terminated: bool,
    pub(super) source_digest: String,
    pub(super) source_json: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::execution_authorization) struct RequestSource {
    pub(in crate::journal::execution_authorization) repository_id: i64,
    pub(in crate::journal::execution_authorization) owner_name: String,
    pub(in crate::journal::execution_authorization) repository_name: String,
    pub(in crate::journal::execution_authorization) workflow_run_id: i64,
    pub(in crate::journal::execution_authorization) event_name: String,
    pub(in crate::journal::execution_authorization) head_branch: Option<String>,
    pub(in crate::journal::execution_authorization) head_sha: String,
    pub(in crate::journal::execution_authorization) job_workflow_ref: String,
    pub(in crate::journal::execution_authorization) workflow: RequestWorkflow,
    pub(in crate::journal::execution_authorization) workflow_source_sha: Option<String>,
    pub(in crate::journal::execution_authorization) run_attempt: i64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::journal::execution_authorization) struct RequestWorkflow {
    pub(in crate::journal::execution_authorization) full_ref: String,
    pub(in crate::journal::execution_authorization) owner_name: String,
    pub(in crate::journal::execution_authorization) repository_name: String,
    pub(in crate::journal::execution_authorization) workflow_path: String,
    pub(in crate::journal::execution_authorization) reference: String,
    pub(in crate::journal::execution_authorization) kind: String,
}

impl StartedExecution {
    pub(super) fn matches_source_request(
        &self,
        execution: &AuthorizedExecutionSource,
    ) -> Result<bool, HostError> {
        execution.validate()?;
        let request = execution.request();
        let Some(repository_id) = i64::try_from(request.repository_id()).ok() else {
            return Ok(false);
        };
        let source = self.validated_source(repository_id)?;
        let Some(workflow_run_id) = i64::try_from(request.run_id()).ok() else {
            return Ok(false);
        };
        let run_attempt = i64::from(request.run_attempt());
        let Some((owner, repository)) = request.repository_name().split_once('/') else {
            return Ok(false);
        };
        let repository_matches = source.owner_name == owner && source.repository_name == repository;
        let Some(native_workflow) = StoredWorkflowRef::parse(&source.job_workflow_ref) else {
            return Ok(false);
        };
        let Some(root_workflow) = StoredWorkflowRef::parse(request.workflow_ref()) else {
            return Ok(false);
        };
        if native_workflow.owner != owner
            || native_workflow.repository != repository
            || root_workflow.owner != owner
            || root_workflow.repository != repository
            || source.workflow.owner_name != native_workflow.owner
            || source.workflow.repository_name != native_workflow.repository
            || source.workflow.workflow_path != native_workflow.path
            || source.workflow.reference != native_workflow.reference
            || source.workflow.full_ref != source.job_workflow_ref
        {
            return Ok(false);
        }
        let root_execution = source.job_workflow_ref == request.workflow_ref();
        let leaf_matches = native_workflow.path == execution.leaf_workflow_path();
        let workflow_source_matches = if root_execution {
            execution.leaf_workflow_path() == execution.root_workflow_path()
                && execution.leaf_commit_sha() == execution.root_commit_sha()
                && source
                    .workflow_source_sha
                    .as_deref()
                    .is_none_or(|sha| sha == execution.root_commit_sha())
        } else {
            source.workflow_source_sha.as_deref() == Some(execution.leaf_commit_sha())
        };
        let head_sha_matches = if request.event_name() == "pull_request" {
            request
                .head_sha()
                .is_some_and(|head_sha| source.head_sha == head_sha)
        } else {
            source.head_sha == request.tested_sha()
        };
        let head_matches = match request.head_ref() {
            Some(head_ref) => source.head_branch.as_deref() == Some(head_ref),
            None if request.event_name() == "push" => request
                .event_ref()
                .strip_prefix("refs/heads/")
                .is_some_and(|branch| source.head_branch.as_deref() == Some(branch)),
            None => true,
        };
        Ok(source.repository_id == repository_id
            && repository_matches
            && source.workflow.owner_name == owner
            && source.workflow.repository_name == repository
            && leaf_matches
            && workflow_source_matches
            && source.workflow_run_id == workflow_run_id
            && source.run_attempt == run_attempt
            && source.event_name == request.event_name()
            && root_workflow.reference == request.event_ref()
            && head_sha_matches
            && head_matches)
    }

    pub(in crate::journal::execution_authorization) fn validated_source(
        &self,
        repository_id: i64,
    ) -> Result<RequestSource, HostError> {
        crate::journal::source_authorization::validate_stored_source(
            &self.source_json,
            repository_id,
        )?;
        let source: RequestSource =
            serde_json::from_str(&self.source_json).map_err(|_| HostError::Journal)?;
        let canonical = serde_json::to_vec(&source).map_err(|_| HostError::Journal)?;
        if canonical != self.source_json.as_bytes() || digest(&canonical) != self.source_digest {
            return Err(HostError::Journal);
        }
        Ok(source)
    }

    pub(in crate::journal::execution_authorization) const fn request_id(&self) -> i64 {
        self.request_id
    }

    pub(in crate::journal::execution_authorization) const fn runner_id(&self) -> i64 {
        self.runner_id
    }

    pub(in crate::journal::execution_authorization) fn runner_name(&self) -> &str {
        &self.runner_name
    }

    pub(in crate::journal::execution_authorization) const fn jit_started_request_id(
        &self,
    ) -> Option<i64> {
        self.jit_started_request_id
    }

    pub(in crate::journal::execution_authorization) const fn jit_witness_request_id(&self) -> i64 {
        self.jit_witness_request_id
    }

    pub(in crate::journal::execution_authorization) const fn jit_runner_id(&self) -> Option<i64> {
        self.jit_runner_id
    }

    pub(in crate::journal::execution_authorization) fn jit_runner_name(&self) -> Option<&str> {
        self.jit_runner_name.as_deref()
    }

    pub(in crate::journal::execution_authorization) fn jit_state(&self) -> &str {
        &self.jit_state
    }

    pub(in crate::journal::execution_authorization) fn acquire_state(&self) -> &str {
        &self.acquire_state
    }

    pub(in crate::journal::execution_authorization) fn native_job_id(&self) -> Option<&str> {
        self.native_job_id.as_deref()
    }

    pub(in crate::journal::execution_authorization) const fn runner_terminated(&self) -> bool {
        self.runner_terminated
    }
}

struct StoredWorkflowRef<'a> {
    owner: &'a str,
    repository: &'a str,
    path: &'a str,
    reference: &'a str,
}

impl<'a> StoredWorkflowRef<'a> {
    fn parse(full_ref: &'a str) -> Option<Self> {
        let (owner, remainder) = full_ref.split_once('/')?;
        let (repository, path_and_reference) = remainder.split_once('/')?;
        let (path, reference) = path_and_reference.rsplit_once('@')?;
        if owner.is_empty() || repository.is_empty() || path.is_empty() || reference.is_empty() {
            return None;
        }
        Some(Self {
            owner,
            repository,
            path,
            reference,
        })
    }
}

pub(in crate::journal::execution_authorization) async fn load(
    connection: &turso::Connection,
    intent_id: i64,
    connection_id: &str,
) -> Result<Option<StartedExecution>, HostError> {
    let mut rows = connection
        .query(
            "SELECT intent.runner_request_id, auth.started_event_digest, auth.started_job_id, auth.started_runner_id, auth.started_runner_name, jit.started_request_id, jit.state, jit.completed_request_id, jit.witness_request_id, auth.acquire_state, auth.completed_event_digest, state.delivery_state, state.resources_bound, state.storage_cleanup_proven, state.instance_binding_digest, intent.cleanup_proven, intent.runner_completed, reservation.state, reservation.remote_cleanup_proven, reservation.local_cleanup_proven, jit.runner_id, jit.runner_name, auth.source_digest, auth.source_json FROM connection_intents AS intent JOIN capacity_reservations AS reservation ON reservation.intent_id = intent.id AND reservation.connection_id = intent.connection_id JOIN connection_runner_jits AS jit ON jit.intent_id = intent.id AND jit.connection_id = intent.connection_id JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id LEFT JOIN connection_request_authorizations AS auth ON auth.connection_id = intent.connection_id AND auth.scale_set_id = intent.scale_set_id AND auth.request_id = intent.runner_request_id WHERE intent.id = ?1 AND intent.connection_id = ?2 AND intent.scale_set_id IS NOT NULL AND reservation.state = 'reserved'",
            (intent_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let started = StartedExecution::from_row(&row)?;
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(started)
}

impl StartedExecution {
    fn from_row(row: &turso::Row) -> Result<Option<Self>, HostError> {
        let request_id: Option<i64> = row.get(0).map_err(|_| HostError::Journal)?;
        let Some(request_id) = request_id else {
            return Ok(None);
        };
        let event_digest: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
        let Some(event_digest) = event_digest else {
            return Ok(None);
        };
        let runner_id: Option<i64> = row.get(3).map_err(|_| HostError::Journal)?;
        let runner_name: Option<String> = row.get(4).map_err(|_| HostError::Journal)?;
        let acquire_state: String = row.get(9).map_err(|_| HostError::Journal)?;
        if acquire_state != "acquired" || !valid_digest(&event_digest) {
            return Ok(None);
        }
        let native_job_id: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
        let jit_started_request_id: Option<i64> = row.get(5).map_err(|_| HostError::Journal)?;
        let jit_state: String = row.get(6).map_err(|_| HostError::Journal)?;
        let jit_completed_request_id: Option<i64> = row.get(7).map_err(|_| HostError::Journal)?;
        let jit_witness_request_id: i64 = row.get(8).map_err(|_| HostError::Journal)?;
        let completed_event_digest: Option<String> = row.get(10).map_err(|_| HostError::Journal)?;
        let state_delivery_state: String = row.get(11).map_err(|_| HostError::Journal)?;
        let state_resources_bound = row.get::<i64>(12).map_err(|_| HostError::Journal)? == 1;
        let state_storage_cleanup_proven = row.get::<i64>(13).map_err(|_| HostError::Journal)? == 1;
        let state_instance_binding_digest: Option<String> =
            row.get(14).map_err(|_| HostError::Journal)?;
        let intent_cleanup_proven = row.get::<i64>(15).map_err(|_| HostError::Journal)? == 1;
        let intent_runner_completed = row.get::<i64>(16).map_err(|_| HostError::Journal)? == 1;
        let reservation_state: String = row.get(17).map_err(|_| HostError::Journal)?;
        let reservation_remote_cleanup_proven =
            row.get::<i64>(18).map_err(|_| HostError::Journal)? == 1;
        let reservation_local_cleanup_proven =
            row.get::<i64>(19).map_err(|_| HostError::Journal)? == 1;
        let jit_runner_id: Option<i64> = row.get(20).map_err(|_| HostError::Journal)?;
        let jit_runner_name: Option<String> = row.get(21).map_err(|_| HostError::Journal)?;
        let source_digest: String = row.get(22).map_err(|_| HostError::Journal)?;
        let source_json: String = row.get(23).map_err(|_| HostError::Journal)?;
        let Some(runner_id) = runner_id else {
            return Ok(None);
        };
        let Some(runner_name) = runner_name else {
            return Ok(None);
        };
        Ok(Some(StartedExecution {
            request_id,
            event_digest,
            native_job_id,
            runner_id,
            runner_name,
            jit_runner_id,
            jit_runner_name,
            jit_started_request_id,
            jit_state,
            jit_completed_request_id,
            jit_witness_request_id,
            acquire_state,
            completed_event_digest,
            state_delivery_state,
            state_resources_bound,
            state_instance_binding_digest,
            reservation_state,
            runner_terminated: state_storage_cleanup_proven
                || intent_cleanup_proven
                || intent_runner_completed
                || reservation_remote_cleanup_proven
                || reservation_local_cleanup_proven,
            source_digest,
            source_json,
        }))
    }
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
