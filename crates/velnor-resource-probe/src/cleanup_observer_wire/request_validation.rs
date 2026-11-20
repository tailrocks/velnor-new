// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};

use super::{CleanupObserverRequest, validate_binding};

pub fn validate_request(request: &CleanupObserverRequest) -> Result<(), &'static str> {
    if request.schema_version != 1
        || !is_lower_hex(&request.operation_id, 32)
        || !is_lower_hex(&request.binding_sha256, 64)
    {
        return Err("cleanup_request_identity");
    }
    validate_binding(&request.binding)?;
    let bytes = serde_json::to_vec(&request.binding).map_err(|_| "cleanup_binding_encode")?;
    if lower_hex(&Sha256::digest(bytes)) != request.binding_sha256 {
        return Err("cleanup_binding_digest");
    }
    Ok(())
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
