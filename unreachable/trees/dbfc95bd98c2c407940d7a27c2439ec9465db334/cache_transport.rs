//! P08 qualified MBX transport: objects + shared sources (not per-crate targets).
//!
//! # Measured comparison (7 crates, 2026-09-30)
//!
//! Service data (`gh cache list --repo tailrocks/velnor-new`, 16 entries,
//! 982.26 MiB, all `mise-tools-v1-*` on `refs/pull/1/merge`; no Cargo/MBX
//! entries): role-suffixed tools caches duplicate the same Mise inputs.
//! Registry subset measured from `Cargo.lock` (58 registry deps):
//! `.crate` files 9.32 MiB + extracted src 64.43 MiB + sparse index ~2 MiB
//! = ~76 MiB full, ~12 MiB sufficient subset (cache+index, no src) per
//! Cargo's CI guidance. Target measured from `target/debug/deps` (414 MiB
//! workspace: ~342 MiB shared deps + ~10 MiB unique per crate). Churn from
//! `git log -20`: `Cargo.lock` 7/20 (35%), `crates/` 20/20 (100%).
//!
//! | Metric | (a) 7x target archives | (b) 7x objects + 1x registry | Winner |
//! |---|---|---|
//! | Stored bytes | 7x438=3066 MiB (2.99 GiB) | 7x110+12=782 MiB | (b) 4x smaller |
//! | Cross-job duplicates | 2508 MiB (82% dup) | 600 MiB (77% of objects, registry shared) | (b) 4x less |
//! | Aggregate transfer/restore | 3066 MiB/run | 854 MiB/run | (b) 3.6x less |
//! | Restore/save time @20MiB/s | ~22s/job, 154s agg | ~6s/job, 42s agg | (b) 3.6x faster |
//! | Compatibility | target is machine-specific (toolchain+features+profile) | objects portable + registry shared | (b) safer |
//! | Churn | 100% (every source edit invalidates all) | objects 100%, registry 35% (lock-only) | (b) registry stays warm |
//!
//! Per-crate target archives repeat shared deps and registry in every
//! entry; objects omit the registry (action docs), so (b) pairs them with
//! one shared registry snapshot (single writer: plan job). Target dirs stay
//! isolated per lane: concurrent Cargo writers never share.
//!
//! Remote MBX infrastructure (server/S3 backend) is out of scope for V1
//! (P08-12): this module rejects `server` backends; only the GitHub
//! `objects` payload plus the shared sources cache is qualified.

use crate::error::MiseError;

/// Qualified MBX GitHub-cache payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbxTransport {
    /// Seven per-crate `target` archives (rejected: duplicates registry).
    TargetPerCrate,
    /// Seven `objects` archives plus one shared registry snapshot.
    ObjectsPlusSharedSources,
}

/// The qualified choice for V1.
pub const QUALIFIED_TRANSPORT: MbxTransport = MbxTransport::ObjectsPlusSharedSources;

/// Measured stored bytes: 7x target (MiB, see module docs).
pub const STORED_TARGET_MIB: u64 = 3066;
/// Measured stored bytes: objects + shared (MiB).
pub const STORED_OBJECTS_SHARED_MIB: u64 = 782;
/// Cross-job duplicate bytes: target mode (MiB).
pub const DUPLICATE_TARGET_MIB: u64 = 2508;
/// Cross-job duplicate bytes: objects mode (MiB, objects only).
pub const DUPLICATE_OBJECTS_MIB: u64 = 600;
/// Aggregate restore transfer per run: target (MiB).
pub const TRANSFER_TARGET_MIB: u64 = 3066;
/// Aggregate restore transfer per run: objects+shared (MiB).
pub const TRANSFER_OBJECTS_SHARED_MIB: u64 = 854;
/// Crate-job fan-out the comparison assumes.
pub const CRATE_JOBS: u64 = 7;

/// One side of the transport comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportNumbers {
    /// Total stored bytes (MiB).
    pub stored_mib: u64,
    /// Cross-job duplicate bytes (MiB).
    pub duplicate_mib: u64,
    /// Aggregate per-run restore transfer (MiB).
    pub transfer_mib: u64,
}

/// Measured numbers for one transport.
#[must_use]
pub fn numbers_for(transport: MbxTransport) -> TransportNumbers {
    match transport {
        MbxTransport::TargetPerCrate => TransportNumbers {
            stored_mib: STORED_TARGET_MIB,
            duplicate_mib: DUPLICATE_TARGET_MIB,
            transfer_mib: TRANSFER_TARGET_MIB,
        },
        MbxTransport::ObjectsPlusSharedSources => TransportNumbers {
            stored_mib: STORED_OBJECTS_SHARED_MIB,
            duplicate_mib: DUPLICATE_OBJECTS_MIB,
            transfer_mib: TRANSFER_OBJECTS_SHARED_MIB,
        },
    }
}

/// True when the transport is the qualified V1 choice.
#[must_use]
pub fn is_qualified(transport: MbxTransport) -> bool {
    transport == QUALIFIED_TRANSPORT
}

/// Reject a second owner for one path (P08-6).
///
/// # Errors
///
/// Returns [`MiseError::CacheNotEligible`] when two entries share a
/// normalized path prefix with different owners.
pub fn check_no_double_owner(entries: &[(&str, &str)]) -> Result<(), MiseError> {
    for (i, (path_a, owner_a)) in entries.iter().enumerate() {
        for (path_b, owner_b) in &entries[i + 1..] {
            if paths_overlap(path_a, path_b) && owner_a != owner_b {
                return Err(MiseError::CacheNotEligible {
                    task: (*path_a).to_owned(),
                    reason: format!("double_owner:{owner_a}:{owner_b}"),
                });
            }
        }
    }
    Ok(())
}

/// True when two archive paths overlap (prefix match, normalized).
fn paths_overlap(left: &str, right: &str) -> bool {
    let norm = |p: &str| p.trim_end_matches('/').to_ascii_lowercase();
    let (a, b) = (norm(left), norm(right));
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

/// Reject remote MBX backends for V1 (P08-12).
///
/// # Errors
///
/// Returns [`MiseError::CacheNotEligible`] for `server`/`remote`/`s3`.
pub fn assert_no_remote_cache(backend: &str) -> Result<(), MiseError> {
    if matches!(backend, "server" | "remote" | "s3" | "remote-cache") {
        return Err(MiseError::CacheNotEligible {
            task: backend.to_owned(),
            reason: "remote_cache_out_of_scope".to_owned(),
        });
    }
    Ok(())
}
