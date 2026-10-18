//! Bounded worker-binding wire records.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::Read;

use crate::ProbeError;

mod observer;
mod records;
mod requests;
mod self_storage;
mod storage;

pub use records::*;
pub use requests::*;

/// Measure storage attachments inside one worker container.
///
/// # Errors
///
/// Returns an error when a requested identity or attachment cannot be verified.
pub fn observe_self_storage(
    request: SelfStorageRequest,
) -> Result<SelfStorageObservation, ProbeError> {
    self_storage::observe(request)
}

/// Measure cgroup ancestry for two owned worker containers.
///
/// # Errors
///
/// Returns an error when a process or cgroup identity cannot be verified.
pub fn observe_owned_cgroups(
    request: OwnedCgroupRequest,
) -> Result<OwnedCgroupObservation, ProbeError> {
    observer::observe(request)
}

/// Maximum output bytes for one worker record.
pub const WORKER_RECORD_LIMIT: usize = 65_536;
/// Maximum input bytes for one worker request.
pub const WORKER_REQUEST_LIMIT: usize = 8_192;
/// Maximum input bytes for one acknowledgment.
pub const WORKER_ACK_LIMIT: usize = 128;
/// Worker wire protocol version.
pub const WORKER_PROTOCOL_VERSION: u16 = 1;
/// Maximum acknowledgment wait in seconds.
pub const ACK_TIMEOUT_SECONDS: u64 = 30;
/// Maximum attachment count in one record.
pub const MAX_ATTACHMENTS: usize = 8;
/// Maximum cgroup node count in one chain.
pub const MAX_CGROUP_NODES: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Worker container role.
pub enum WorkerRole {
    /// Official GitHub runner container.
    Runner,
    /// Private Docker-in-Docker container.
    Dind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Fixed storage attachment role.
pub enum AttachmentRole {
    /// Runner root filesystem.
    RunnerRoot,
    /// Shared runner home filesystem.
    RunnerHome,
    /// Shared runner work filesystem.
    RunnerWork,
    /// Runner temporary filesystem.
    RunnerTmp,
    /// Runner shared-memory filesystem.
    RunnerShm,
    /// Shared runtime filesystem.
    SharedRun,
    /// Runner job-facts filesystem.
    JobFacts,
    /// Runner runtime identity filesystem.
    RuntimeIdentity,
    /// Selected daemon Docker-root filesystem.
    EngineMetadata,
    /// `DinD` root filesystem.
    DindRoot,
    /// `DinD` data filesystem.
    DindData,
    /// `DinD` temporary filesystem.
    DindTmp,
    /// `DinD` shared-memory filesystem.
    DindShm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Fixed role and absolute target path.
pub struct AttachmentSpec {
    /// Fixed role for this observation.
    pub role: AttachmentRole,
    /// Absolute path inside the worker container.
    pub target: &'static str,
}

#[cfg(any(target_os = "linux", test))]
pub(crate) const RUNNER_ATTACHMENTS: [AttachmentSpec; 8] = [
    AttachmentSpec {
        role: AttachmentRole::RunnerRoot,
        target: "/",
    },
    AttachmentSpec {
        role: AttachmentRole::RunnerHome,
        target: "/home/runner",
    },
    AttachmentSpec {
        role: AttachmentRole::RunnerWork,
        target: "/home/runner/_work",
    },
    AttachmentSpec {
        role: AttachmentRole::RunnerTmp,
        target: "/tmp",
    },
    AttachmentSpec {
        role: AttachmentRole::RunnerShm,
        target: "/dev/shm",
    },
    AttachmentSpec {
        role: AttachmentRole::SharedRun,
        target: "/run",
    },
    AttachmentSpec {
        role: AttachmentRole::JobFacts,
        target: "/opt/velnor/job-facts",
    },
    AttachmentSpec {
        role: AttachmentRole::RuntimeIdentity,
        target: "/.velnor-runtime-identity",
    },
];

#[cfg(any(target_os = "linux", test))]
pub(crate) const DIND_ATTACHMENTS: [AttachmentSpec; 7] = [
    AttachmentSpec {
        role: AttachmentRole::DindRoot,
        target: "/",
    },
    AttachmentSpec {
        role: AttachmentRole::RunnerHome,
        target: "/home/runner",
    },
    AttachmentSpec {
        role: AttachmentRole::RunnerWork,
        target: "/home/runner/_work",
    },
    AttachmentSpec {
        role: AttachmentRole::SharedRun,
        target: "/run",
    },
    AttachmentSpec {
        role: AttachmentRole::DindData,
        target: "/var/lib/docker",
    },
    AttachmentSpec {
        role: AttachmentRole::DindTmp,
        target: "/tmp",
    },
    AttachmentSpec {
        role: AttachmentRole::DindShm,
        target: "/dev/shm",
    },
];

/// Read one canonical JSON line within a fixed byte limit.
///
/// # Errors
///
/// Returns an error when the line is absent, too large, invalid, or not canonical.
pub fn read_compact_line<T>(input: &mut impl Read, limit: usize) -> Result<T, ProbeError>
where
    T: DeserializeOwned + Serialize,
{
    let mut bytes = Vec::with_capacity(limit.min(4096));
    let mut byte = [0_u8; 1];
    loop {
        match input.read(&mut byte) {
            Ok(0) => return Err(ProbeError::Invalid("worker_line_missing")),
            Ok(1) if byte[0] == b'\n' => break,
            Ok(1) => {
                bytes.push(byte[0]);
                if bytes.len() > limit {
                    return Err(ProbeError::InputTooLarge("worker_line"));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Ok(_) | Err(_) => return Err(ProbeError::Invalid("worker_read")),
        }
    }
    let value: T =
        serde_json::from_slice(&bytes).map_err(|_| ProbeError::Invalid("worker_json"))?;
    if serde_json::to_vec(&value).map_err(|_| ProbeError::Invalid("worker_json"))? != bytes {
        return Err(ProbeError::Invalid("worker_json_not_canonical"));
    }
    Ok(value)
}

/// Validate one acknowledgment for the waiting worker operation.
///
/// # Errors
///
/// Returns an error when the line, size, identity, or operation ID is invalid.
pub fn validate_acknowledgment(bytes: &[u8], operation_id: &str) -> Result<(), ProbeError> {
    if bytes.len() > WORKER_ACK_LIMIT
        || bytes.last() != Some(&b'\n')
        || bytes[..bytes.len().saturating_sub(1)].contains(&b'\n')
    {
        return Err(ProbeError::Invalid("ack_line_format"));
    }
    let acknowledgment: AcknowledgeRequest =
        read_compact_line(&mut bytes.to_vec().as_slice(), WORKER_ACK_LIMIT)?;
    validate_operation_id(&acknowledgment.operation_id)?;
    if acknowledgment.operation_id != operation_id {
        return Err(ProbeError::Invalid("ack_operation_mismatch"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Request that releases a waiting helper.
pub struct AcknowledgeRequest {
    /// Fresh 32-character lower-case hexadecimal identity.
    pub operation_id: String,
}

/// Validate a 32-character lower-case hexadecimal operation ID.
///
/// # Errors
///
/// Returns an error when the value is not 32 lower-case hexadecimal characters.
pub(crate) fn validate_operation_id(value: &str) -> Result<(), ProbeError> {
    if value.len() != 32 || !lower_hex(value) {
        return Err(ProbeError::Invalid("operation_id"));
    }
    Ok(())
}

/// Validate a 64-character lower-case hexadecimal container ID.
///
/// # Errors
///
/// Returns an error when the value is not 64 lower-case hexadecimal characters.
pub(crate) fn validate_container_id(value: &str) -> Result<(), ProbeError> {
    if value.len() != 64 || !lower_hex(value) {
        return Err(ProbeError::Invalid("container_id"));
    }
    Ok(())
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn request() -> SelfStorageRequest {
        SelfStorageRequest {
            schema_version: 1,
            operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
            container_id: "a".repeat(64),
            role: WorkerRole::Runner,
        }
    }

    #[test]
    fn request_ids_require_fixed_lowercase_hex() {
        assert!(validate_operation_id("a".repeat(32).as_str()).is_ok());
        assert!(validate_operation_id("A".repeat(32).as_str()).is_err());
        assert!(validate_container_id("f".repeat(64).as_str()).is_ok());
        assert!(validate_container_id("f".repeat(63).as_str()).is_err());
    }

    #[test]
    fn compact_request_rejects_spaces_and_unknown_fields() {
        let request = request();
        let encoded = serde_json::to_vec(&request).unwrap_or_default();
        assert_ne!(encoded, [] as [u8; 0]);
        let valid = format!("{}\n", String::from_utf8_lossy(&encoded));
        let parsed = read_compact_line::<SelfStorageRequest>(
            &mut Cursor::new(valid.as_bytes()),
            WORKER_REQUEST_LIMIT,
        );
        assert!(parsed.is_ok_and(|value| value == request.clone()));
        assert!(
            read_compact_line::<SelfStorageRequest>(
                &mut Cursor::new(valid.replace(',', ", ").as_bytes()),
                WORKER_REQUEST_LIMIT,
            )
            .is_err()
        );
        let unknown = format!("{}\n", valid.trim_end().replace('}', ",\"extra\":0}"));
        assert!(
            read_compact_line::<SelfStorageRequest>(
                &mut Cursor::new(unknown.as_bytes()),
                WORKER_REQUEST_LIMIT,
            )
            .is_err()
        );
    }

    #[test]
    fn acknowledgment_requires_one_canonical_matching_operation() {
        let operation = "0123456789abcdef0123456789abcdef";
        let valid = format!("{{\"operation_id\":\"{operation}\"}}\n");
        assert!(validate_acknowledgment(valid.as_bytes(), operation).is_ok());
        assert!(validate_acknowledgment(valid.as_bytes(), "f".repeat(32).as_str()).is_err());
        assert!(
            validate_acknowledgment(format!("{}{{}}", valid.trim_end()).as_bytes(), operation,)
                .is_err()
        );
        assert!(validate_acknowledgment(format!("{valid}\n").as_bytes(), operation,).is_err());
    }

    #[test]
    fn role_tables_are_fixed_and_have_no_duplicate_role_target_pairs() {
        assert_eq!(RUNNER_ATTACHMENTS.len(), 8);
        assert_eq!(DIND_ATTACHMENTS.len(), 7);
        assert_eq!(RUNNER_ATTACHMENTS[6].target, "/opt/velnor/job-facts");
        assert_eq!(RUNNER_ATTACHMENTS[7].target, "/.velnor-runtime-identity");
        assert_eq!(DIND_ATTACHMENTS[4].target, "/var/lib/docker");
        for specs in [&RUNNER_ATTACHMENTS[..], &DIND_ATTACHMENTS[..]] {
            for (index, spec) in specs.iter().enumerate() {
                assert!(
                    !specs[index + 1..]
                        .iter()
                        .any(|other| other.target == spec.target)
                );
            }
        }
    }
}
