//! Lock-against-manifest verification (split from `lock`: size gate).

use velnor_actions_contract_release::{GeneratorLock, ReleaseManifest, ReleaseTarget};

use super::{
    MISE_VERSION,
    lock::{LockError, mismatch},
};

/// Verify lock against manifest: same version, same source commit,
/// equal URL+SHA per target.
///
/// Also pins the lock's Mise bootstrap record to the compiled [`MISE_VERSION`].
/// # Errors
pub fn verify_lock_against_manifest(
    lock: &GeneratorLock,
    manifest: &ReleaseManifest,
) -> Result<(), LockError> {
    if lock.generator.version != manifest.version {
        return Err(mismatch(format!(
            "version:{}:{}",
            lock.generator.version, manifest.version
        )));
    }
    if lock.generator.commit != manifest.commit {
        return Err(mismatch(format!(
            "commit:{}:{}",
            lock.generator.commit, manifest.commit
        )));
    }
    for target in ReleaseTarget::ALL {
        let locked = lock
            .binary_for_target(target.triple())
            .ok_or_else(|| mismatch(format!("missing_target:{}", target.triple())))?;
        let released = manifest
            .record_for_target(target.triple())
            .ok_or_else(|| mismatch(format!("manifest_missing_target:{}", target.triple())))?;
        if locked.artifact != released.artifact || locked.sha256 != released.sha256 {
            return Err(mismatch(format!("target_diverged:{}", target.triple())));
        }
    }
    if lock.mise_bootstrap.version != MISE_VERSION {
        return Err(mismatch(format!(
            "mise_bootstrap:{}:{MISE_VERSION}",
            lock.mise_bootstrap.version
        )));
    }
    Ok(())
}
