// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_readback_verdicts() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let key = generation.cache_key();
    let bytes = fixture_bytes();
    let digest = sha256_hex(&bytes);
    let size = fixture_size()?;
    let entry = fixture_entry(&generation, size);
    let happy = bound_inputs(&key, &digest, size, Some(&bytes), Some(&entry), true);
    match decide_readback(&happy).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Verified {
            archive_digest,
            stored_bytes,
        } => {
            if archive_digest != digest || stored_bytes != size {
                return Err(fail("verified readback drifted from frozen bytes"));
            }
        }
        _ => return Err(fail("exact restored bytes missed verification")),
    }
    let stale = bound_inputs(&key, &digest, size, Some(&bytes), Some(&entry), false);
    match decide_readback(&stale).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Unavailable { reason } => {
            if reason != READBACK_UNKNOWN_INVENTORY {
                return Err(fail("inventory gate missed its reason"));
            }
        }
        _ => return Err(fail("unreconciled inventory passed readback")),
    }
    let missing_entry = bound_inputs(&key, &digest, size, Some(&bytes), None, true);
    match decide_readback(&missing_entry).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Unavailable { reason } => {
            if reason != READBACK_ENTRY_NOT_OBSERVED {
                return Err(fail("missing entry missed its reason"));
            }
        }
        _ => return Err(fail("missing entry passed readback")),
    }
    let missing_bytes = bound_inputs(&key, &digest, size, None, Some(&entry), true);
    match decide_readback(&missing_bytes).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Unavailable { reason } => {
            if reason != READBACK_NOT_AVAILABLE {
                return Err(fail("missing readback was called corrupt"));
            }
        }
        _ => return Err(fail("missing readback passed verification")),
    }
    Ok(())
}

fn check_readback_corrupt() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let key = generation.cache_key();
    let bytes = fixture_bytes();
    let digest = sha256_hex(&bytes);
    let size = fixture_size()?;
    let entry = fixture_entry(&generation, size);
    let mut mutated = bytes.clone();
    mutated[0] ^= 0xff;
    let mutated_inputs = bound_inputs(&key, &digest, size, Some(&mutated), Some(&entry), true);
    match decide_readback(&mutated_inputs).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Corrupt { evidence } => {
            if evidence != READBACK_RESTORED_DIGEST_MISMATCH {
                return Err(fail("mutated bytes missed their evidence"));
            }
        }
        _ => return Err(fail("mutated bytes passed verification")),
    }
    let short = bound_inputs(
        &key,
        &digest,
        size,
        Some(&bytes[..bytes.len() / 2]),
        Some(&entry),
        true,
    );
    match decide_readback(&short).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Corrupt { evidence } => {
            if evidence != READBACK_RESTORED_SIZE_MISMATCH {
                return Err(fail("truncated bytes missed their evidence"));
            }
        }
        _ => return Err(fail("truncated bytes passed verification")),
    }
    let wrong_size = fixture_entry(&generation, size + 1);
    let size_inputs = bound_inputs(&key, &digest, size, Some(&bytes), Some(&wrong_size), true);
    match decide_readback(&size_inputs).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Corrupt { evidence } => {
            if evidence != READBACK_STORED_BYTES_MISMATCH {
                return Err(fail("stored size drift missed its evidence"));
            }
        }
        _ => return Err(fail("stored size drift passed verification")),
    }
    Ok(())
}

fn check_readback_binding() -> Result<(), Box<dyn Error>> {
    let generation = fixture_generation()?;
    let key = generation.cache_key();
    let other = GenerationId {
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t26-foreign"))?,
        ..fixture_generation()?
    };
    let other_key = other.cache_key();
    let bytes = fixture_bytes();
    let digest = sha256_hex(&bytes);
    let size = fixture_size()?;
    let entry = fixture_entry(&generation, size);
    let foreign_key = bound_inputs(&other_key, &digest, size, Some(&bytes), Some(&entry), true);
    match decide_readback(&foreign_key).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Corrupt { evidence } => {
            if evidence != READBACK_KEY_MISMATCH {
                return Err(fail("foreign key missed its evidence"));
            }
        }
        _ => return Err(fail("foreign key passed verification")),
    }
    let mut foreign_ref = entry.clone();
    "refs/pull/9/merge".clone_into(&mut foreign_ref.ref_name);
    let ref_inputs = bound_inputs(&key, &digest, size, Some(&bytes), Some(&foreign_ref), true);
    match decide_readback(&ref_inputs).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Corrupt { evidence } => {
            if evidence != READBACK_REF_MISMATCH {
                return Err(fail("foreign ref missed its evidence"));
            }
        }
        _ => return Err(fail("foreign ref passed verification")),
    }
    let mut foreign_version = entry.clone();
    "v0".clone_into(&mut foreign_version.version);
    let version_inputs = bound_inputs(
        &key,
        &digest,
        size,
        Some(&bytes),
        Some(&foreign_version),
        true,
    );
    match decide_readback(&version_inputs).map_err(|error| fail(&error.to_string()))? {
        ReadbackVerdict::Corrupt { evidence } => {
            if evidence != READBACK_VERSION_MISMATCH {
                return Err(fail("foreign version missed its evidence"));
            }
        }
        _ => return Err(fail("foreign version passed verification")),
    }
    Ok(())
}

fn check_recovery_helpers() -> Result<(), Box<dyn Error>> {
    validate_sha256_hex(&sha256_hex(b"gc1-t26-probe")).map_err(|error| fail(&error.to_string()))?;
    require_empty_restore_target(false).map_err(|error| fail(&error.to_string()))?;
    let generation = fixture_generation()?;
    let key = generation.cache_key();
    let older = GenerationId {
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t26-older"))?,
        ..fixture_generation()?
    };
    let newer = GenerationId {
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t26-newer"))?,
        ..fixture_generation()?
    };
    let older_key = older.cache_key();
    let newer_key = newer.cache_key();
    let retained =
        retain_usable_families(&[newer_key.clone(), key.clone(), older_key.clone()], &key)
            .map_err(|error| fail(&error.to_string()))?;
    let mut expected = vec![older_key.clone(), newer_key.clone()];
    expected.sort();
    if retained != expected {
        return Err(fail("usable families were not retained unchanged"));
    }
    if retained.contains(&key) {
        return Err(fail("failed family survived retention"));
    }
    Ok(())
}

#[test]
fn gc1_t26_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_save_only_missing_complete_admitted()?;
    check_verified_round_trip()?;
    check_five_outcomes()?;
    check_readback_verdicts()?;
    check_readback_corrupt()?;
    check_readback_binding()?;
    check_recovery_helpers()?;
    Ok(())
}
