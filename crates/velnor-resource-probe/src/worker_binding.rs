// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::Read;

use crate::ProbeError;

pub(in crate::worker_binding) mod observer;
mod records;
mod requests;
mod self_storage;
mod storage;

pub use records::*;
pub use requests::*;

pub fn observe_self_storage(
    request: SelfStorageRequest,
) -> Result<SelfStorageObservation, ProbeError> {
    self_storage::observe(request)
}

pub fn observe_owned_cgroups(
    request: &OwnedCgroupRequest,
) -> Result<OwnedCgroupObservation, ProbeError> {
    observer::observe(request)
}

pub const WORKER_RECORD_LIMIT: usize = 65_536;

pub const WORKER_REQUEST_LIMIT: usize = 8_192;

pub const WORKER_ACK_LIMIT: usize = 128;

pub const WORKER_PROTOCOL_VERSION: u16 = 3;

pub const ACK_TIMEOUT_SECONDS: u64 = 30;

pub const MAX_ATTACHMENTS: usize = 10;

pub const MAX_CGROUP_NODES: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerRole {
    Runner,

    Dind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentRole {
    RunnerRoot,

    RunnerHome,

    RunnerWork,

    RunnerTmp,

    RunnerShm,

    SharedRun,

    JobFacts,

    RuntimeIdentity,

    SourceProofControl,

    SourceProofInbox,

    EngineMetadata,

    DindRoot,

    DindData,

    DindTmp,

    DindShm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentSpec {
    pub role: AttachmentRole,

    pub target: &'static str,
}

#[cfg(target_os = "linux")]
pub(crate) const RUNNER_ATTACHMENTS: [AttachmentSpec; 10] = [
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
    AttachmentSpec {
        role: AttachmentRole::SourceProofControl,
        target: "/opt/velnor/source-proof/control",
    },
    AttachmentSpec {
        role: AttachmentRole::SourceProofInbox,
        target: "/opt/velnor/source-proof/inbox",
    },
];

#[cfg(target_os = "linux")]
pub(crate) const DIND_ATTACHMENTS: [AttachmentSpec; 9] = [
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
    AttachmentSpec {
        role: AttachmentRole::JobFacts,
        target: "/opt/velnor/job-facts",
    },
    AttachmentSpec {
        role: AttachmentRole::RuntimeIdentity,
        target: "/.velnor-runtime-identity",
    },
];

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
pub struct AcknowledgeRequest {
    pub operation_id: String,
}

pub(crate) fn validate_operation_id(value: &str) -> Result<(), ProbeError> {
    if value.len() != 32 || !lower_hex(value) {
        return Err(ProbeError::Invalid("operation_id"));
    }
    Ok(())
}

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
