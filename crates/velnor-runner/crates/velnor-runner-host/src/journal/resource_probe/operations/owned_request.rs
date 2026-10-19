//! Make sure that owned probe requests and probe work have the correct format.

use super::{
    Digest, HostError, HostHelperRole, ProbeSeed, ProbeWork, ResourceProbePurpose, Sha256,
    capacity_request_sha256,
};

pub(in crate::journal) fn seed_valid(seed: &ProbeSeed) -> bool {
    hex_string(&seed.operation_id, 32)
        && hex_string(&seed.instance_id, 32)
        && !seed.engine_id.is_empty()
        && seed.engine_id.len() <= 256
        && !seed.engine_id.chars().any(char::is_control)
        && hex_string(&seed.docker_root_digest, 64)
        && hex_string(&seed.source_revision, 40)
        && seed.runtime_image_id.starts_with("sha256:")
        && hex_string(seed.runtime_image_id.trim_start_matches("sha256:"), 64)
        && hex_string(&seed.image_binding_digest, 64)
        && seed.operation_name == format!("velnor-resource-probe-{}", seed.operation_id)
        && hex_string(&seed.projection_digest, 64)
        && lower_hex(&seed.request_sha256, 64)
        && (seed.purpose != ResourceProbePurpose::Capacity
            || seed.request_sha256 == capacity_request_sha256())
}

pub(super) fn owned_request_valid(seed: &ProbeSeed, request_json: &str) -> bool {
    seed.purpose == ResourceProbePurpose::OwnedCgroups
        && owned_request_matches(&seed.operation_id, request_json, &seed.request_sha256)
}

pub(super) fn owned_request_matches(operation_id: &str, request_json: &str, digest: &str) -> bool {
    if request_json.is_empty()
        || request_json.len() > velnor_resource_probe::worker_binding::WORKER_REQUEST_LIMIT
        || request_json.contains('\n')
    {
        return false;
    }
    let Ok(request) = serde_json::from_str::<
        velnor_resource_probe::worker_binding::OwnedCgroupRequest,
    >(request_json) else {
        return false;
    };
    if request.validate().is_err() || request.operation_id != operation_id {
        return false;
    }
    let Ok(canonical) = serde_json::to_vec(&request) else {
        return false;
    };
    if canonical != request_json.as_bytes() {
        return false;
    }
    let mut hasher = Sha256::new();
    hasher.update(b"velnor-owned-cgroups-request-v1\0");
    hasher.update(request_json.as_bytes());
    hasher.update(b"\n");
    lower_hex(digest, 64) && encode_hex(&hasher.finalize()) == digest
}

pub(super) fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

pub(super) fn lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn decode_work(
    operation_id: &str,
    role: HostHelperRole,
    purpose: &str,
    request_sha256: Option<String>,
    request_context: Option<String>,
) -> Result<ProbeWork, HostError> {
    match role {
        HostHelperRole::ResourceProbe => {
            let purpose = ResourceProbePurpose::parse(purpose)?;
            let request_sha256 = request_sha256.ok_or(HostError::Journal)?;
            if !lower_hex(&request_sha256, 64) {
                return Err(HostError::Journal);
            }
            match (purpose, request_context.as_deref()) {
                (ResourceProbePurpose::Capacity, None)
                    if request_sha256 == capacity_request_sha256() => {}
                (ResourceProbePurpose::OwnedCgroups, Some(context))
                    if owned_request_matches(operation_id, context, &request_sha256) => {}
                _ => return Err(HostError::Journal),
            }
            Ok(ProbeWork::ResourceProbe {
                purpose,
                request_sha256,
                request_context,
            })
        }
        HostHelperRole::ExecutionPublication => {
            if purpose != "none" || request_sha256.is_some() || request_context.is_some() {
                return Err(HostError::Journal);
            }
            Ok(ProbeWork::ExecutionPublication)
        }
    }
}

pub(super) fn hex_string(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn container_id_valid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
