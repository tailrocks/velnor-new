use crate::error::MiseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbxTransport {
    TargetPerCrate,
    ObjectsPlusSharedSources,
}

pub const QUALIFIED_TRANSPORT: MbxTransport = MbxTransport::ObjectsPlusSharedSources;

pub const STORED_TARGET_MIB: u64 = 3066;
pub const STORED_OBJECTS_SHARED_MIB: u64 = 782;
pub const DUPLICATE_TARGET_MIB: u64 = 2508;
pub const DUPLICATE_OBJECTS_MIB: u64 = 600;
pub const TRANSFER_TARGET_MIB: u64 = 3066;
pub const TRANSFER_OBJECTS_SHARED_MIB: u64 = 854;
pub const CRATE_JOBS: u64 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportNumbers {
    pub stored_mib: u64,
    pub duplicate_mib: u64,
    pub transfer_mib: u64,
}

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

#[must_use]
pub fn is_qualified(transport: MbxTransport) -> bool {
    transport == QUALIFIED_TRANSPORT
}

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

fn paths_overlap(left: &str, right: &str) -> bool {
    let norm = |p: &str| p.trim_end_matches('/').to_ascii_lowercase();
    let (a, b) = (norm(left), norm(right));
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

pub fn assert_no_remote_cache(backend: &str) -> Result<(), MiseError> {
    if matches!(backend, "server" | "remote" | "s3" | "remote-cache") {
        return Err(MiseError::CacheNotEligible {
            task: backend.to_owned(),
            reason: "remote_cache_out_of_scope".to_owned(),
        });
    }
    Ok(())
}
