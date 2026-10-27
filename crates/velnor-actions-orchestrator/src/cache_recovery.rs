// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use sha2::{Digest as Sha2Digest, Sha256};
use velnor_actions_contract::workflow::cache_identity::GenerationId;

pub const MAX_RECOVERY_TEXT_BYTES: usize = 512;

pub const READBACK_UNKNOWN_INVENTORY: &str = "inventory-incomplete";

pub const READBACK_ENTRY_NOT_OBSERVED: &str = "entry-not-observed";

pub const READBACK_NOT_AVAILABLE: &str = "readback-not-available";

pub const READBACK_KEY_MISMATCH: &str = "key-mismatch";

pub const READBACK_REF_MISMATCH: &str = "ref-mismatch";

pub const READBACK_VERSION_MISMATCH: &str = "version-mismatch";

pub const READBACK_STORED_BYTES_MISMATCH: &str = "stored-bytes-mismatch";

pub const READBACK_RESTORED_DIGEST_MISMATCH: &str = "restored-digest-mismatch";

pub const READBACK_RESTORED_SIZE_MISMATCH: &str = "restored-size-mismatch";

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cache_recovery: {problem}")]
pub struct RecoveryError {
    pub problem: String,
}

fn recovery_error(problem: &str) -> RecoveryError {
    RecoveryError {
        problem: problem.to_owned(),
    }
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

pub fn validate_sha256_hex(value: &str) -> Result<(), RecoveryError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(recovery_error("recovery_digest_invalid"));
    }
    Ok(())
}

fn require_text(field: &str, value: &str) -> Result<(), RecoveryError> {
    if value.is_empty()
        || value.len() > MAX_RECOVERY_TEXT_BYTES
        || value.bytes().any(|byte| byte == 0)
    {
        return Err(recovery_error(&format!("{field}_invalid")));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEntry {
    pub repository_id: u64,
    pub cache_id: u64,
    pub ref_name: String,
    pub key: String,
    pub version: String,
    pub size_in_bytes: u64,
    pub created_at: String,
}

impl ObservedEntry {
    pub fn validate(&self) -> Result<(), RecoveryError> {
        if self.repository_id == 0 {
            return Err(recovery_error("recovery_entry_repository_invalid"));
        }
        require_text("recovery.entry.ref_name", &self.ref_name)?;
        require_text("recovery.entry.key", &self.key)?;
        require_text("recovery.entry.version", &self.version)?;
        require_text("recovery.entry.created_at", &self.created_at)?;
        GenerationId::parse_key(&self.key)
            .map_err(|_| recovery_error("recovery_entry_key_malformed"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadbackVerdict {
    Verified {
        archive_digest: String,
        stored_bytes: u64,
    },
    Corrupt {
        evidence: String,
    },
    Unavailable {
        reason: String,
    },
}

pub struct ReadbackInputs<'a> {
    pub frozen_archive_digest: &'a str,
    pub frozen_archive_bytes: u64,
    pub restored_bytes: Option<&'a [u8]>,
    pub inventory_complete: bool,
    pub observed_entry: Option<&'a ObservedEntry>,
    pub expected_key: &'a str,
    pub expected_ref: &'a str,
    pub expected_version: &'a str,
}

fn decide_inner(inputs: &ReadbackInputs<'_>) -> ReadbackVerdict {
    if !inputs.inventory_complete {
        return ReadbackVerdict::Unavailable {
            reason: READBACK_UNKNOWN_INVENTORY.to_owned(),
        };
    }
    let Some(entry) = inputs.observed_entry else {
        return ReadbackVerdict::Unavailable {
            reason: READBACK_ENTRY_NOT_OBSERVED.to_owned(),
        };
    };
    if entry.key != inputs.expected_key {
        return ReadbackVerdict::Corrupt {
            evidence: READBACK_KEY_MISMATCH.to_owned(),
        };
    }
    if entry.ref_name != inputs.expected_ref {
        return ReadbackVerdict::Corrupt {
            evidence: READBACK_REF_MISMATCH.to_owned(),
        };
    }
    if entry.version != inputs.expected_version {
        return ReadbackVerdict::Corrupt {
            evidence: READBACK_VERSION_MISMATCH.to_owned(),
        };
    }
    if entry.size_in_bytes != inputs.frozen_archive_bytes {
        return ReadbackVerdict::Corrupt {
            evidence: READBACK_STORED_BYTES_MISMATCH.to_owned(),
        };
    }
    let Some(restored) = inputs.restored_bytes else {
        return ReadbackVerdict::Unavailable {
            reason: READBACK_NOT_AVAILABLE.to_owned(),
        };
    };
    let restored_len = u64::try_from(restored.len()).unwrap_or(u64::MAX);
    if restored_len != inputs.frozen_archive_bytes {
        return ReadbackVerdict::Corrupt {
            evidence: READBACK_RESTORED_SIZE_MISMATCH.to_owned(),
        };
    }
    let digest = sha256_hex(restored);
    if digest != inputs.frozen_archive_digest {
        return ReadbackVerdict::Corrupt {
            evidence: READBACK_RESTORED_DIGEST_MISMATCH.to_owned(),
        };
    }
    ReadbackVerdict::Verified {
        archive_digest: digest,
        stored_bytes: inputs.frozen_archive_bytes,
    }
}

pub fn decide_readback(inputs: &ReadbackInputs<'_>) -> Result<ReadbackVerdict, RecoveryError> {
    validate_sha256_hex(inputs.frozen_archive_digest)?;
    if inputs.frozen_archive_bytes == 0 {
        return Err(recovery_error("recovery_frozen_empty"));
    }
    require_text("recovery.expected_key", inputs.expected_key)?;
    require_text("recovery.expected_ref", inputs.expected_ref)?;
    require_text("recovery.expected_version", inputs.expected_version)?;
    if let Some(entry) = inputs.observed_entry {
        entry.validate()?;
    }
    Ok(decide_inner(inputs))
}

pub fn require_empty_restore_target(target_occupied: bool) -> Result<(), RecoveryError> {
    if target_occupied {
        return Err(recovery_error("recovery_target_not_empty"));
    }
    Ok(())
}

pub fn retain_usable_families(
    usable_keys: &[String],
    failed_key: &str,
) -> Result<Vec<String>, RecoveryError> {
    GenerationId::parse_key(failed_key)
        .map_err(|_| recovery_error("recovery_failed_key_malformed"))?;
    let mut retained = Vec::with_capacity(usable_keys.len());
    for key in usable_keys {
        GenerationId::parse_key(key)
            .map_err(|_| recovery_error("recovery_usable_key_malformed"))?;
        if key != failed_key {
            retained.push(key.clone());
        }
    }
    retained.sort();
    retained.dedup();
    Ok(retained)
}
