//! Validate each persisted challenge, channel, and source record.

use crate::error::HostError;
use crate::launch::execution_authorization::{ExecutionProofChallenge, PersistedExecutionSource};

use super::super::channels::ProofChannelIdentity;

pub(super) async fn validate_proof_rows(connection: &turso::Connection) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT intent_id, reservation_id, connection_id, scale_set_id, witness_request_id, generation, policy_digest, engine_id, docker_root_digest, runner_id, runner_name, runner_container_id, dind_container_id, instance_binding_digest, challenge, channel_digest, challenge_state, challenge_json, challenge_digest, source_digest, source_json, proof_state, actual_request_id, started_event_digest, run_id, run_attempt, workflow_job_key, authority_role, rest_job_id, channel_json FROM connection_execution_proofs ORDER BY intent_id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        ProofRow::from_turso(&row)?.validate(connection).await?;
    }
    Ok(())
}

struct ProofRow {
    intent_id: i64,
    reservation_id: i64,
    connection_id: String,
    scale_set_id: i64,
    witness_request_id: i64,
    generation: i64,
    policy_digest: String,
    engine_id: String,
    docker_root_digest: String,
    runner_id: i64,
    runner_name: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    challenge_text: String,
    channel_digest: String,
    challenge_state: String,
    challenge_json: String,
    challenge_digest: String,
    source_digest: Option<String>,
    source_json: Option<String>,
    proof_state: String,
    actual_request_id: Option<i64>,
    started_event_digest: Option<String>,
    run_id: Option<i64>,
    run_attempt: Option<i64>,
    job_key: Option<String>,
    role: Option<String>,
    rest_job_id: Option<String>,
    channel_json: String,
}

impl ProofRow {
    fn from_turso(row: &turso::Row) -> Result<Self, HostError> {
        Ok(Self {
            intent_id: row.get(0).map_err(|_| HostError::Journal)?,
            reservation_id: row.get(1).map_err(|_| HostError::Journal)?,
            connection_id: row.get(2).map_err(|_| HostError::Journal)?,
            scale_set_id: row.get(3).map_err(|_| HostError::Journal)?,
            witness_request_id: row.get(4).map_err(|_| HostError::Journal)?,
            generation: row.get(5).map_err(|_| HostError::Journal)?,
            policy_digest: row.get(6).map_err(|_| HostError::Journal)?,
            engine_id: row.get(7).map_err(|_| HostError::Journal)?,
            docker_root_digest: row.get(8).map_err(|_| HostError::Journal)?,
            runner_id: row.get(9).map_err(|_| HostError::Journal)?,
            runner_name: row.get(10).map_err(|_| HostError::Journal)?,
            runner_container_id: row.get(11).map_err(|_| HostError::Journal)?,
            dind_container_id: row.get(12).map_err(|_| HostError::Journal)?,
            instance_binding_digest: row.get(13).map_err(|_| HostError::Journal)?,
            challenge_text: row.get(14).map_err(|_| HostError::Journal)?,
            channel_digest: row.get(15).map_err(|_| HostError::Journal)?,
            challenge_state: row.get(16).map_err(|_| HostError::Journal)?,
            challenge_json: row.get(17).map_err(|_| HostError::Journal)?,
            challenge_digest: row.get(18).map_err(|_| HostError::Journal)?,
            source_digest: row.get(19).map_err(|_| HostError::Journal)?,
            source_json: row.get(20).map_err(|_| HostError::Journal)?,
            proof_state: row.get(21).map_err(|_| HostError::Journal)?,
            actual_request_id: row.get(22).map_err(|_| HostError::Journal)?,
            started_event_digest: row.get(23).map_err(|_| HostError::Journal)?,
            run_id: row.get(24).map_err(|_| HostError::Journal)?,
            run_attempt: row.get(25).map_err(|_| HostError::Journal)?,
            job_key: row.get(26).map_err(|_| HostError::Journal)?,
            role: row.get(27).map_err(|_| HostError::Journal)?,
            rest_job_id: row.get(28).map_err(|_| HostError::Journal)?,
            channel_json: row.get(29).map_err(|_| HostError::Journal)?,
        })
    }

    async fn validate(&self, connection: &turso::Connection) -> Result<(), HostError> {
        validate_challenge(connection, self).await?;
        super::super::channels::validate(
            connection,
            &self.channel_json,
            &self.channel_digest,
            &ProofChannelIdentity {
                reservation_id: self.reservation_id,
                intent_id: self.intent_id,
                connection_id: &self.connection_id,
                scale_set_id: self.scale_set_id,
                witness_request_id: self.witness_request_id,
                engine_id: &self.engine_id,
                docker_root_digest: &self.docker_root_digest,
                runner_id: self.runner_id,
                runner_name: &self.runner_name,
                runner_container_id: &self.runner_container_id,
                dind_container_id: &self.dind_container_id,
                instance_binding_digest: &self.instance_binding_digest,
            },
        )
        .await?;
        validate_source(connection, self).await
    }
}

async fn validate_challenge(
    connection: &turso::Connection,
    proof: &ProofRow,
) -> Result<(), HostError> {
    let challenge: ExecutionProofChallenge =
        serde_json::from_str(&proof.challenge_json).map_err(|_| HostError::Journal)?;
    let canonical = super::super::canonical_json(&challenge)?;
    if canonical != proof.challenge_json.as_bytes()
        || super::super::digest(&canonical) != proof.challenge_digest
        || challenge.challenge() != proof.challenge_text
        || challenge.channel_digest() != proof.channel_digest
        || challenge.intent_id() != proof.intent_id
        || challenge.reservation_id() != proof.reservation_id
        || challenge.witness_request_id() != proof.witness_request_id
        || challenge.runner_id() != proof.runner_id
        || challenge.runner_name() != proof.runner_name
        || challenge.runner_scale_set_id() != proof.scale_set_id
        || challenge.runner_container_id() != proof.runner_container_id
        || challenge.dind_container_id() != proof.dind_container_id
        || challenge.engine_id() != proof.engine_id
        || challenge.docker_root_digest() != proof.docker_root_digest
        || challenge.instance_binding_digest() != proof.instance_binding_digest
        || i64::try_from(challenge.policy_generation()).ok() != Some(proof.generation)
        || challenge.policy_digest() != proof.policy_digest
        || !challenge.valid()
    {
        return Err(HostError::Journal);
    }
    validate_challenge_file(connection, &proof.challenge_state, &proof.challenge_json).await
}

async fn validate_challenge_file(
    connection: &turso::Connection,
    state: &str,
    challenge_json: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT challenge_file_path, challenge_file_inode, challenge_file_bytes, challenge_file_uid, challenge_file_gid, challenge_file_mode, challenge_mount_id, challenge_device_major, challenge_device_minor, challenge_statx_device_major, challenge_statx_device_minor, challenge_fs_type_magic, challenge_filesystem_id, challenge_filesystem_instance_digest, challenge_read_only, challenge_no_copy FROM connection_execution_proofs WHERE challenge_json = ?1",
            [challenge_json],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Err(HostError::Journal);
    };
    let path: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
    let inode: Option<i64> = row.get(1).map_err(|_| HostError::Journal)?;
    let bytes: Option<i64> = row.get(2).map_err(|_| HostError::Journal)?;
    let uid: Option<i64> = row.get(3).map_err(|_| HostError::Journal)?;
    let gid: Option<i64> = row.get(4).map_err(|_| HostError::Journal)?;
    let mode: Option<i64> = row.get(5).map_err(|_| HostError::Journal)?;
    let mount_id: Option<i64> = row.get(6).map_err(|_| HostError::Journal)?;
    let device_major: Option<i64> = row.get(7).map_err(|_| HostError::Journal)?;
    let device_minor: Option<i64> = row.get(8).map_err(|_| HostError::Journal)?;
    let statx_major: Option<i64> = row.get(9).map_err(|_| HostError::Journal)?;
    let statx_minor: Option<i64> = row.get(10).map_err(|_| HostError::Journal)?;
    let fs_magic: Option<String> = row.get(11).map_err(|_| HostError::Journal)?;
    let fs_id: Option<String> = row.get(12).map_err(|_| HostError::Journal)?;
    let fs_digest: Option<String> = row.get(13).map_err(|_| HostError::Journal)?;
    let read_only: Option<i64> = row.get(14).map_err(|_| HostError::Journal)?;
    let no_copy: Option<i64> = row.get(15).map_err(|_| HostError::Journal)?;
    validate_file_receipt(
        state,
        challenge_json,
        ChallengeFileReceipt {
            path: path.as_deref(),
            inode,
            bytes,
            uid,
            gid,
            mode,
            mount_id,
            device_major,
            device_minor,
            statx_major,
            statx_minor,
            fs_magic: fs_magic.as_deref(),
            fs_id: fs_id.as_deref(),
            fs_digest: fs_digest.as_deref(),
            read_only,
            no_copy,
        },
        rows.next().await.map_err(|_| HostError::Journal)?.is_some(),
    )
}

#[derive(Clone, Copy)]
struct ChallengeFileReceipt<'a> {
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

fn validate_file_receipt(
    state: &str,
    challenge_json: &str,
    receipt: ChallengeFileReceipt<'_>,
    duplicate: bool,
) -> Result<(), HostError> {
    let exists = receipt.inode.is_some();
    if duplicate
        || (state == "published" && !exists)
        || (matches!(state, "not_requested" | "requested" | "uncertain") && exists)
    {
        return Err(HostError::Journal);
    }
    if !exists {
        return Ok(());
    }
    let parsed_magic = receipt
        .fs_magic
        .and_then(|value| u64::from_str_radix(value, 16).ok());
    if receipt.path != Some("/opt/velnor/source-proof/control/challenge.json")
        || receipt.inode.is_none_or(|value| value <= 0)
        || receipt.bytes
            != Some(i64::try_from(challenge_json.len()).map_err(|_| HostError::Journal)?)
        || receipt.uid != Some(0)
        || receipt.gid != Some(0)
        || receipt.mode != Some(0o444)
        || receipt.mount_id.is_none_or(|value| value <= 0)
        || receipt.fs_id.is_none_or(|value| !super::valid_hex16(value))
        || receipt
            .fs_digest
            .is_none_or(|value| !super::valid_digest(value))
        || receipt.read_only != Some(1)
        || receipt.no_copy != Some(1)
        || ((receipt.device_major, receipt.device_minor)
            != (receipt.statx_major, receipt.statx_minor)
            && parsed_magic != Some(0x9123_683e))
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_source(
    connection: &turso::Connection,
    proof: &ProofRow,
) -> Result<(), HostError> {
    if !source_fields_are_paired(proof.source_digest.as_deref(), proof.source_json.as_deref()) {
        return Err(HostError::Journal);
    }
    match (proof.source_digest.as_deref(), proof.source_json.as_deref()) {
        (None, None) if source_absence_is_valid(&proof.proof_state, proof.actual_request_id) => {
            Ok(())
        }
        (Some(expected), Some(json)) => {
            validate_persisted_source(connection, proof, expected, json).await
        }
        _ => Err(HostError::Journal),
    }
}

async fn validate_persisted_source(
    connection: &turso::Connection,
    proof: &ProofRow,
    expected_digest: &str,
    json: &str,
) -> Result<(), HostError> {
    let source: PersistedExecutionSource =
        serde_json::from_str(json).map_err(|_| HostError::Journal)?;
    let canonical = source.canonical_json()?;
    if canonical != json.as_bytes()
        || super::super::digest(&canonical) != expected_digest
        || source.validate().is_err()
        || source.request().challenge() != proof.challenge_text
        || source.request().reservation_id() != proof.reservation_id
        || source.request().intent_id() != proof.intent_id
    {
        return Err(HostError::Journal);
    }
    if proof.proof_state != "approved" {
        return Ok(());
    }
    validate_approved_source(connection, proof, &source).await
}

async fn validate_approved_source(
    connection: &turso::Connection,
    proof: &ProofRow,
    source: &PersistedExecutionSource,
) -> Result<(), HostError> {
    if proof.actual_request_id.is_none_or(|value| value <= 0)
        || proof
            .started_event_digest
            .as_deref()
            .is_none_or(|value| !super::valid_digest(value))
        || proof.run_id.and_then(|value| u64::try_from(value).ok())
            != Some(source.request().run_id())
        || proof
            .run_attempt
            .and_then(|value| u32::try_from(value).ok())
            != Some(source.request().run_attempt())
        || proof.job_key.as_deref() != Some(source.request().job_key())
        || proof.generation <= 0
        || !super::valid_digest(&proof.policy_digest)
        || !matches!(
            proof.role.as_deref(),
            Some("verification" | "trusted_publication")
        )
        || proof.rest_job_id.as_deref() != source.rest_job_id()
    {
        return Err(HostError::Journal);
    }
    let (policy, repository) = super::policy_snapshot(
        connection,
        &proof.connection_id,
        proof.generation,
        &proof.policy_digest,
    )
    .await?;
    let authority_role = source
        .recorded_policy_role(&policy, &repository)
        .map(super::role_name)
        .ok_or(HostError::Journal)?;
    if proof.role.as_deref() != Some(authority_role) {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn source_absence_is_valid(proof_state: &str, actual_request_id: Option<i64>) -> bool {
    matches!(proof_state, "pending" | "denied") && actual_request_id.is_none()
}

fn source_fields_are_paired(digest: Option<&str>, json: Option<&str>) -> bool {
    digest.is_some() == json.is_some()
}

#[cfg(test)]
mod tests {
    use super::{source_absence_is_valid, source_fields_are_paired};

    #[test]
    fn absent_source_is_allowed_only_before_request_or_after_denial() {
        assert!(source_absence_is_valid("pending", None));
        assert!(source_absence_is_valid("denied", None));
        assert!(!source_absence_is_valid("approved", None));
        assert!(!source_absence_is_valid("pending", Some(7)));
        assert!(!source_absence_is_valid("denied", Some(7)));
    }

    #[test]
    fn source_digest_and_json_must_be_present_together() {
        assert!(source_fields_are_paired(None, None));
        assert!(source_fields_are_paired(Some("digest"), Some("json")));
        assert!(!source_fields_are_paired(Some("digest"), None));
        assert!(!source_fields_are_paired(None, Some("json")));
    }
}
