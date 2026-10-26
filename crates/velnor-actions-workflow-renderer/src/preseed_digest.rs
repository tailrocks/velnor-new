// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};

use crate::RenderError;

use super::PRESEED_BUILD_OUTPUT;

const SOURCE_BOUND_HELPER_DIGEST_TAG: &str = "preseed-helper-inputs-v1";

#[must_use]
pub fn source_bound_helper_digest_input(
    build: &[String],
    target: &str,
    toolchain: &str,
) -> Vec<u8> {
    let mut input = Vec::new();
    for part in [
        SOURCE_BOUND_HELPER_DIGEST_TAG,
        PRESEED_BUILD_OUTPUT,
        target,
        toolchain,
    ] {
        input.extend_from_slice(part.as_bytes());
        input.push(0);
    }
    for arg in build {
        input.extend_from_slice(arg.as_bytes());
        input.push(0);
    }
    input
}

pub fn source_bound_helper_digest(build: &[String], target: &str) -> Result<String, RenderError> {
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "preseed_unsupported_target:{target}"
        )));
    }
    let toolchain = crate::candidate::toolchain_identity(build)?;
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(source_bound_helper_digest_input(build, target, &toolchain)) {
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        digest.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    Ok(digest)
}
