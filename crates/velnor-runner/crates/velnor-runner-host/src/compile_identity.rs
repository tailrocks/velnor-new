// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompiledReleaseIdentity {
    source_sha: [u8; 20],
    helper_sha256: [u8; 32],
}

impl CompiledReleaseIdentity {
    pub(crate) const fn source_sha(&self) -> &[u8; 20] {
        &self.source_sha
    }

    pub(crate) const fn helper_sha256(&self) -> &[u8; 32] {
        &self.helper_sha256
    }
}

const COMPILED_RELEASE_IDENTITY: Option<CompiledReleaseIdentity> = match (
    option_env!("VELNOR_RUNNER_BUILD_SOURCE_SHA"),
    option_env!("VELNOR_RUNNER_BUILD_HELPER_SHA256"),
) {
    (None, None) => None,
    (Some(source), Some(helper)) => Some(CompiledReleaseIdentity {
        source_sha: match parse_hex::<20>(source) {
            Some(value) => value,
            None => panic!("runner source digest must be 40 lowercase hexadecimal characters"),
        },
        helper_sha256: match parse_hex::<32>(helper) {
            Some(value) => value,
            None => panic!("runner helper digest must be 64 lowercase hexadecimal characters"),
        },
    }),
    _ => panic!("runner source and helper digests must be supplied together"),
};

const fn parse_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    let bytes = value.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let mut result = [0; N];
    let mut index = 0;
    while index < N {
        let Some(high) = hex_digit(bytes[index * 2]) else {
            return None;
        };
        let Some(low) = hex_digit(bytes[index * 2 + 1]) else {
            return None;
        };
        result[index] = (high << 4) | low;
        index += 1;
    }
    Some(result)
}

const fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

pub(crate) const fn compiled_release_identity() -> Option<CompiledReleaseIdentity> {
    COMPILED_RELEASE_IDENTITY
}
