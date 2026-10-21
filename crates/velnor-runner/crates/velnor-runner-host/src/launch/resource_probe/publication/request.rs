use velnor_resource_probe::{
    execution_publication::{ExecutionPublicationContext, ExecutionPublicationRecord},
    job_facts_wire::{JobFactsPublicationContext, JobFactsPublicationRecord},
};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) enum PublicationRequest {
    Control(ExecutionPublicationContext),
    JobFacts(JobFactsPublicationContext),
}

pub(super) enum PublicationRecord {
    Control(Box<ExecutionPublicationRecord>),
    JobFacts(Box<JobFactsPublicationRecord>),
}

impl From<ExecutionPublicationContext> for PublicationRequest {
    fn from(value: ExecutionPublicationContext) -> Self {
        Self::Control(value)
    }
}

impl PublicationRequest {
    pub(super) fn operation_id(&self) -> &str {
        match self {
            Self::Control(value) => &value.operation_id,
            Self::JobFacts(value) => &value.operation_id,
        }
    }

    pub(super) fn payload_sha256(&self) -> &str {
        match self {
            Self::Control(value) => &value.payload_sha256,
            Self::JobFacts(value) => &value.payload_sha256,
        }
    }

    pub(super) fn guest_boot_id(&self) -> &str {
        match self {
            Self::Control(value) => &value.expected_guest_boot_id,
            Self::JobFacts(value) => &value.expected_guest_boot_id,
        }
    }

    pub(super) fn directory(
        &self,
    ) -> &velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory {
        match self {
            Self::Control(value) => &value.expected_runner_control,
            Self::JobFacts(value) => &value.expected_runner_job_facts,
        }
    }

    pub(super) fn mode(&self) -> &'static str {
        match self {
            Self::Control(_) => "execution_publication",
            Self::JobFacts(_) => "job_facts_publication",
        }
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, crate::error::HostError> {
        match self {
            Self::Control(value) => serde_json::to_vec(value),
            Self::JobFacts(value) => serde_json::to_vec(value),
        }
        .map_err(|_| crate::error::HostError::Frame)
    }

    pub(super) fn validate(&self) -> Result<(), crate::error::HostError> {
        let (version, hex, target, limit) = match self {
            Self::Control(value) => (
                value.schema_version,
                value.payload_hex.as_str(),
                value.expected_runner_control.target.as_str(),
                velnor_resource_probe::execution_publication::PUBLICATION_REQUEST_LIMIT,
            ),
            Self::JobFacts(value) => (
                value.schema_version,
                value.payload_hex.as_str(),
                value.expected_runner_job_facts.target.as_str(),
                velnor_resource_probe::job_facts::JOB_FACTS_REQUEST_LIMIT,
            ),
        };
        let encoded = self.encode()?;
        let expected_version = match self {
            Self::Control(_) => 1,
            Self::JobFacts(_) => 2,
        };
        if version != expected_version
            || !lower_hex(self.operation_id(), 32)
            || !valid_boot_id(self.guest_boot_id())
            || !lower_hex(self.payload_sha256(), 64)
            || hex.is_empty()
            || hex.len() % 2 != 0
            || hex.len() / 2 > 64 * 1024
            || hex
                .bytes()
                .any(|byte| !(byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
            || target != self.target()
            || encoded.len() > limit
        {
            return Err(crate::error::HostError::Identity);
        }
        let payload = decode_lower_hex(hex)?;
        match self {
            Self::Control(_) if digest(&payload) != self.payload_sha256() => {
                return Err(crate::error::HostError::Identity);
            }
            Self::JobFacts(value) => {
                let guest = decode_lower_hex(&value.guest_image_hex)?;
                if digest(&payload) != value.job_facts_sha256
                    || digest(&guest) != value.guest_image_sha256
                    || !lower_hex(&value.publication_binding_sha256, 64)
                    || !lower_hex(&value.payload_sha256, 64)
                    || velnor_resource_probe::job_facts_wire::paired_payload_sha256(value)
                        .map_err(|_| crate::error::HostError::Frame)?
                        != value.payload_sha256
                {
                    return Err(crate::error::HostError::Identity);
                }
            }
            Self::Control(_) => {}
        }
        Ok(())
    }

    fn target(&self) -> &'static str {
        match self {
            Self::Control(_) => "/opt/velnor/source-proof/control",
            Self::JobFacts(_) => "/opt/velnor/job-facts",
        }
    }

    pub(super) fn decode_record(
        &self,
        bytes: &[u8],
    ) -> Result<PublicationRecord, crate::error::HostError> {
        let record = match self {
            Self::Control(_) => serde_json::from_slice(bytes)
                .map(Box::new)
                .map(PublicationRecord::Control)
                .map_err(|_| crate::error::HostError::Frame),
            Self::JobFacts(_) => serde_json::from_slice(bytes)
                .map(Box::new)
                .map(PublicationRecord::JobFacts)
                .map_err(|_| crate::error::HostError::Frame),
        }?;
        let matched = match (&record, self) {
            (PublicationRecord::Control(record), Self::Control(request)) => {
                record.schema_version == 1
                    && record.operation_id == request.operation_id
                    && record.guest_boot_id == request.expected_guest_boot_id
                    && record.kind == request.kind
                    && record.payload_sha256 == request.payload_sha256
            }
            (PublicationRecord::JobFacts(record), Self::JobFacts(request)) => {
                record.schema_version == 2
                    && record.operation_id == request.operation_id
                    && record.guest_boot_id == request.expected_guest_boot_id
                    && record.payload_sha256 == request.payload_sha256
                    && record.guest_image_sha256 == request.guest_image_sha256
                    && record.publication_binding_sha256 == request.publication_binding_sha256
                    && record.file.readback_sha256 == request.job_facts_sha256
                    && record.guest_image_file.readback_sha256 == request.guest_image_sha256
                    && record.mount_namespace_id > 0
            }
            _ => false,
        };
        if !matched {
            return Err(crate::error::HostError::Identity);
        }
        Ok(record)
    }
}

pub(super) fn job_facts_request(
    operation_id: &str,
    guest_boot_id: &str,
    payload: &[u8],
    directory: &velnor_resource_probe::execution_channel_wire::ExecutionChannelDirectory,
    guest_image: &[u8],
    runtime_identity: velnor_resource_probe::job_facts_wire::JobFactsRuntimeIdentityDirectory,
    publication_binding_sha256: &str,
) -> Result<JobFactsPublicationContext, crate::error::HostError> {
    if payload.is_empty()
        || payload.len() > 64 * 1024
        || guest_image.is_empty()
        || guest_image.len() > 16 * 1024
        || operation_id.len() != 32
        || !operation_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || directory.target != "/opt/velnor/job-facts"
        || directory.inode == 0
        || directory.owner_uid != 0
        || directory.owner_gid != 0
        || directory.mode != 0o555
        || !directory.read_only
        || !lower_hex(publication_binding_sha256, 64)
    {
        return Err(crate::error::HostError::Identity);
    }
    let context = JobFactsPublicationContext {
        schema_version: 2,
        operation_id: operation_id.to_owned(),
        expected_guest_boot_id: guest_boot_id.to_owned(),
        payload_hex: encode_hex(payload),
        job_facts_sha256: digest(payload),
        payload_sha256: String::new(),
        guest_image_hex: encode_hex(guest_image),
        guest_image_sha256: digest(guest_image),
        publication_binding_sha256: publication_binding_sha256.to_owned(),
        expected_runner_job_facts: directory.clone(),
        expected_runner_runtime_identity: runtime_identity,
    };
    let mut context = context;
    context.payload_sha256 = velnor_resource_probe::job_facts_wire::paired_payload_sha256(&context)
        .map_err(|_| crate::error::HostError::Frame)?;
    let encoded = serde_json::to_vec(&context).map_err(|_| crate::error::HostError::Frame)?;
    if encoded.len() > velnor_resource_probe::job_facts::JOB_FACTS_REQUEST_LIMIT {
        return Err(crate::error::HostError::Frame);
    }
    Ok(context)
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    encode_hex(&Sha256::digest(bytes))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

fn decode_lower_hex(value: &str) -> Result<Vec<u8>, crate::error::HostError> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(crate::error::HostError::Identity);
    }
    let mut decoded = Vec::with_capacity(pairs.len());
    for [high, low] in pairs {
        let high = hex_digit(*high).ok_or(crate::error::HostError::Identity)?;
        let low = hex_digit(*low).ok_or(crate::error::HostError::Identity)?;
        decoded.push((high << 4) | low);
    }
    Ok(decoded)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_boot_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
            }
        })
}
