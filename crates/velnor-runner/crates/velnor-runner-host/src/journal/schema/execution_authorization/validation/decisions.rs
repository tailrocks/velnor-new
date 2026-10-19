//! Prepared, delivered, and uncertain decision validation.

use crate::error::HostError;
use crate::launch::execution_authorization::{
    PersistedExecutionDecision, PersistedExecutionSource,
};

use super::{canonical_json, count, digest};

pub(super) async fn validate(connection: &turso::Connection) -> Result<(), HostError> {
    let invalid = count(
        connection,
        "SELECT COUNT(*) FROM connection_execution_decisions AS decision LEFT JOIN connection_execution_proofs AS proof ON proof.intent_id = decision.intent_id LEFT JOIN connection_request_authorizations AS auth ON auth.connection_id = decision.connection_id AND auth.scale_set_id = decision.scale_set_id AND auth.request_id = decision.actual_request_id WHERE proof.intent_id IS NULL OR proof.reservation_id != decision.reservation_id OR proof.connection_id != decision.connection_id OR proof.scale_set_id != decision.scale_set_id OR proof.witness_request_id != decision.witness_request_id OR proof.actual_request_id != decision.actual_request_id OR proof.challenge != decision.challenge OR proof.generation != decision.generation OR proof.policy_digest != decision.policy_digest OR proof.proof_state NOT IN ('approved', 'denied') OR proof.challenge_state NOT IN ('published', 'abandoned') OR auth.request_id IS NULL OR auth.acquire_state != 'acquired' OR auth.started_event_digest IS NOT proof.started_event_digest OR auth.started_runner_id != proof.runner_id OR auth.started_runner_name != proof.runner_name OR auth.completed_event_digest IS NOT NULL OR length(CAST(decision.decision_json AS BLOB)) > 8192 OR decision.allow != CASE proof.proof_state WHEN 'approved' THEN 1 ELSE 0 END OR (decision.delivery_state = 'delivered' AND decision.decision_file_inode IS NULL) OR (decision.delivery_state != 'delivered' AND decision.decision_file_inode IS NOT NULL)",
    )
    .await?;
    if invalid != 0 {
        return Err(HostError::Journal);
    }
    validate_decision_rows(connection).await
}

async fn validate_decision_rows(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT decision.intent_id, decision.reservation_id, decision.connection_id, decision.scale_set_id, decision.witness_request_id, decision.actual_request_id, decision.challenge, decision.generation, decision.policy_digest, decision.request_digest, decision.allow, decision.decision_digest, decision.decision_json, decision.delivery_state, decision.decision_file_path, decision.decision_file_inode, decision.decision_file_bytes, decision.decision_file_uid, decision.decision_file_gid, decision.decision_file_mode, decision.decision_mount_id, decision.decision_device_major, decision.decision_device_minor, decision.decision_statx_device_major, decision.decision_statx_device_minor, decision.decision_fs_type_magic, decision.decision_filesystem_id, decision.decision_filesystem_instance_digest, decision.decision_read_only, decision.decision_no_copy, proof.runner_id, proof.runner_name, proof.source_json, proof.native_job_id, proof.rest_job_id FROM connection_execution_decisions AS decision JOIN connection_execution_proofs AS proof ON proof.intent_id = decision.intent_id ORDER BY decision.intent_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        DecisionRow::from_turso(&row)?.validate()?;
    }
    Ok(())
}

struct DecisionRow {
    intent_id: i64,
    reservation_id: i64,
    actual_request_id: i64,
    challenge: String,
    generation: i64,
    policy_digest: String,
    request_digest: String,
    allow: bool,
    decision_digest: String,
    decision_json: String,
    delivery_state: String,
    file_path: Option<String>,
    file_inode: Option<i64>,
    file_bytes: Option<i64>,
    file_uid: Option<i64>,
    file_gid: Option<i64>,
    file_mode: Option<i64>,
    mount_id: Option<i64>,
    device_major: Option<i64>,
    device_minor: Option<i64>,
    statx_major: Option<i64>,
    statx_minor: Option<i64>,
    fs_magic: Option<String>,
    fs_id: Option<String>,
    fs_digest: Option<String>,
    read_only: Option<i64>,
    no_copy: Option<i64>,
    runner_id: i64,
    source_json: Option<String>,
    native_job_id: Option<String>,
    rest_job_id: Option<String>,
}

impl DecisionRow {
    fn from_turso(row: &turso::Row) -> Result<Self, HostError> {
        Ok(Self {
            intent_id: row.get(0).map_err(|_| HostError::Journal)?,
            reservation_id: row.get(1).map_err(|_| HostError::Journal)?,
            actual_request_id: row.get(5).map_err(|_| HostError::Journal)?,
            challenge: row.get(6).map_err(|_| HostError::Journal)?,
            generation: row.get(7).map_err(|_| HostError::Journal)?,
            policy_digest: row.get(8).map_err(|_| HostError::Journal)?,
            request_digest: row.get(9).map_err(|_| HostError::Journal)?,
            allow: row.get::<i64>(10).map_err(|_| HostError::Journal)? == 1,
            decision_digest: row.get(11).map_err(|_| HostError::Journal)?,
            decision_json: row.get(12).map_err(|_| HostError::Journal)?,
            delivery_state: row.get(13).map_err(|_| HostError::Journal)?,
            file_path: row.get(14).map_err(|_| HostError::Journal)?,
            file_inode: row.get(15).map_err(|_| HostError::Journal)?,
            file_bytes: row.get(16).map_err(|_| HostError::Journal)?,
            file_uid: row.get(17).map_err(|_| HostError::Journal)?,
            file_gid: row.get(18).map_err(|_| HostError::Journal)?,
            file_mode: row.get(19).map_err(|_| HostError::Journal)?,
            mount_id: row.get(20).map_err(|_| HostError::Journal)?,
            device_major: row.get(21).map_err(|_| HostError::Journal)?,
            device_minor: row.get(22).map_err(|_| HostError::Journal)?,
            statx_major: row.get(23).map_err(|_| HostError::Journal)?,
            statx_minor: row.get(24).map_err(|_| HostError::Journal)?,
            fs_magic: row.get(25).map_err(|_| HostError::Journal)?,
            fs_id: row.get(26).map_err(|_| HostError::Journal)?,
            fs_digest: row.get(27).map_err(|_| HostError::Journal)?,
            read_only: row.get(28).map_err(|_| HostError::Journal)?,
            no_copy: row.get(29).map_err(|_| HostError::Journal)?,
            runner_id: row.get(30).map_err(|_| HostError::Journal)?,
            source_json: row.get(32).map_err(|_| HostError::Journal)?,
            native_job_id: row.get(33).map_err(|_| HostError::Journal)?,
            rest_job_id: row.get(34).map_err(|_| HostError::Journal)?,
        })
    }

    fn validate(&self) -> Result<(), HostError> {
        validate_decision_json(DecisionJson {
            json: &self.decision_json,
            expected_digest: &self.decision_digest,
            request_digest: &self.request_digest,
            challenge: &self.challenge,
            allow: self.allow,
            reservation_id: self.reservation_id,
            intent_id: self.intent_id,
            actual_request_id: self.actual_request_id,
            runner_id: self.runner_id,
            generation: self.generation,
            policy_digest: &self.policy_digest,
            native_job_id: self.native_job_id.as_deref(),
            rest_job_id: self.rest_job_id.as_deref(),
            source_json: self.source_json.as_deref(),
        })?;
        validate_decision_file(DecisionFile {
            state: &self.delivery_state,
            json: &self.decision_json,
            path: self.file_path.as_deref(),
            inode: self.file_inode,
            bytes: self.file_bytes,
            uid: self.file_uid,
            gid: self.file_gid,
            mode: self.file_mode,
            mount_id: self.mount_id,
            device_major: self.device_major,
            device_minor: self.device_minor,
            statx_major: self.statx_major,
            statx_minor: self.statx_minor,
            fs_magic: self.fs_magic.as_deref(),
            fs_id: self.fs_id.as_deref(),
            fs_digest: self.fs_digest.as_deref(),
            read_only: self.read_only,
            no_copy: self.no_copy,
        })
    }
}

#[derive(Clone, Copy)]
struct DecisionJson<'a> {
    json: &'a str,
    expected_digest: &'a str,
    request_digest: &'a str,
    challenge: &'a str,
    allow: bool,
    reservation_id: i64,
    intent_id: i64,
    actual_request_id: i64,
    runner_id: i64,
    generation: i64,
    policy_digest: &'a str,
    native_job_id: Option<&'a str>,
    rest_job_id: Option<&'a str>,
    source_json: Option<&'a str>,
}

fn validate_decision_json(fields: DecisionJson<'_>) -> Result<(), HostError> {
    let decision: PersistedExecutionDecision =
        serde_json::from_str(fields.json).map_err(|_| HostError::Journal)?;
    let canonical = canonical_json(&decision)?;
    let source: PersistedExecutionSource =
        serde_json::from_str(fields.source_json.ok_or(HostError::Journal)?)
            .map_err(|_| HostError::Journal)?;
    if canonical != fields.json.as_bytes()
        || digest(&canonical) != fields.expected_digest
        || decision.sha256()? != fields.expected_digest
        || decision.challenge() != fields.challenge
        || decision.request_digest() != fields.request_digest
        || decision.allow() != fields.allow
        || decision.reservation_id() != fields.reservation_id
        || decision.intent_id() != fields.intent_id
        || decision.actual_request_id() != fields.actual_request_id
        || decision.runner_id() != fields.runner_id
        || decision.policy_generation()
            != u64::try_from(fields.generation).map_err(|_| HostError::Journal)?
        || decision.policy_digest() != fields.policy_digest
        || decision.native_job_id() != fields.native_job_id
        || decision.rest_job_id() != fields.rest_job_id
        || decision.run_id() != source.request().run_id()
        || decision.run_attempt() != source.request().run_attempt()
        || decision.job_key() != source.request().job_key()
        || decision.request_digest() != source.request().sha256()?.as_str()
        || !decision.valid()
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct DecisionFile<'a> {
    state: &'a str,
    json: &'a str,
    path: Option<&'a str>,
    inode: Option<i64>,
    bytes: Option<i64>,
    uid: Option<i64>,
    gid: Option<i64>,
    mode: Option<i64>,
    mount_id: Option<i64>,
    device_major: Option<i64>,
    device_minor: Option<i64>,
    statx_major: Option<i64>,
    statx_minor: Option<i64>,
    fs_magic: Option<&'a str>,
    fs_id: Option<&'a str>,
    fs_digest: Option<&'a str>,
    read_only: Option<i64>,
    no_copy: Option<i64>,
}

fn validate_decision_file(fields: DecisionFile<'_>) -> Result<(), HostError> {
    let exists = fields.inode.is_some();
    if (fields.state == "delivered") != exists
        || (exists
            && (fields.path != Some("/opt/velnor/source-proof/control/decision.json")
                || fields.inode.is_none_or(|value| value <= 0)
                || fields.bytes
                    != Some(i64::try_from(fields.json.len()).map_err(|_| HostError::Journal)?)
                || fields.uid != Some(0)
                || fields.gid != Some(0)
                || fields.mode != Some(0o444)
                || fields.mount_id.is_none_or(|value| value <= 0)
                || fields.read_only != Some(1)
                || fields.no_copy != Some(1)
                || fields
                    .fs_magic
                    .and_then(|value| u64::from_str_radix(value, 16).ok())
                    .is_none_or(|value| value == 0)
                || fields.fs_id.is_none_or(|value| !valid_hex16(value))
                || fields.fs_digest.is_none_or(|value| !valid_digest(value))
                || ((fields.device_major != fields.statx_major
                    || fields.device_minor != fields.statx_minor)
                    && fields
                        .fs_magic
                        .and_then(|value| u64::from_str_radix(value, 16).ok())
                        != Some(0x9123_683e))))
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_hex16(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
