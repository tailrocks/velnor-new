use sha2::{Digest, Sha256};

use crate::{ProbeError, job_facts_wire::JobFactsPublicationContext};

const DIRECTORY_TARGET: &str = "/opt/velnor/job-facts";
const PAYLOAD_LIMIT: usize = 64 * 1024;
const ROOT_UID: u32 = 0;
const ROOT_GID: u32 = 0;
const DIRECTORY_MODE: u32 = 0o555;

pub(super) fn validate_context(context: &JobFactsPublicationContext) -> Result<(), ProbeError> {
    if context.schema_version != 1
        || !valid_operation_id(&context.operation_id)
        || !valid_boot_id(&context.expected_guest_boot_id)
        || !valid_digest(&context.payload_sha256)
        || context.payload_hex.is_empty()
        || context.payload_hex.len() > PAYLOAD_LIMIT * 2
        || !context.payload_hex.len().is_multiple_of(2)
        || !context
            .payload_hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProbeError::Invalid("job_facts_request"));
    }
    let expected = &context.expected_runner_job_facts;
    if expected.target != DIRECTORY_TARGET
        || expected.inode == 0
        || expected.owner_uid != ROOT_UID
        || expected.owner_gid != ROOT_GID
        || expected.mode != DIRECTORY_MODE
        || !expected.read_only
        || expected.mount_id == 0
        || expected.statx_mount_id == 0
        || expected.filesystem_id == 0
        || expected.mount_source.is_empty()
        || expected.mount_root.is_empty()
    {
        return Err(ProbeError::Invalid("job_facts_runner_directory"));
    }
    let bytes = decode_payload(&context.payload_hex)?;
    if lower_hex(&Sha256::digest(&bytes)) != context.payload_sha256 {
        return Err(ProbeError::Invalid("job_facts_payload_digest"));
    }
    let document = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|_| ProbeError::Invalid("job_facts_payload_json"))?;
    if !valid_document_shape(&document) {
        return Err(ProbeError::Invalid("job_facts_payload_json"));
    }
    Ok(())
}

pub(super) fn valid_document_shape(document: &serde_json::Value) -> bool {
    let Some(object) = document.as_object() else {
        return false;
    };
    let keys = [
        "schema",
        "version",
        "scope",
        "reservation",
        "runner",
        "engine",
        "profile",
        "sample",
        "instance_binding",
        "instance_binding_digest",
        "cache",
        "job_facts_mount",
        "projection_digest",
    ];
    object.len() == keys.len()
        && keys.iter().all(|key| object.contains_key(*key))
        && object.get("schema").and_then(serde_json::Value::as_str)
            == Some("velnor.storage-cache-admission")
        && object.get("version").and_then(serde_json::Value::as_u64) == Some(1)
}

pub(super) fn decode_payload(value: &str) -> Result<Vec<u8>, ProbeError> {
    if !value.len().is_multiple_of(2) || value.len() > PAYLOAD_LIMIT * 2 {
        return Err(ProbeError::InputTooLarge("job_facts_payload"));
    }
    let mut output = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().as_chunks::<2>().0 {
        output.push(
            (decode_hex(pair[0]).ok_or(ProbeError::Invalid("job_facts_payload_hex"))? << 4)
                | decode_hex(pair[1]).ok_or(ProbeError::Invalid("job_facts_payload_hex"))?,
        );
    }
    Ok(output)
}

fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn valid_operation_id(value: &str) -> bool {
    value.len() == 32
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
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
