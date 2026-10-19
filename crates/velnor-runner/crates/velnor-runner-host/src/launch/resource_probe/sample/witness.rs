use std::time::Instant;

#[cfg(test)]
use std::time::Duration;

use super::super::record::{InodePolicy, ProbeRecord};
use super::MAX_OBSERVATION_AGE;

/// Immutable facts from the selected guest Docker-root mount sample.
#[derive(Debug, Clone)]
pub(crate) struct ProbeStorageMountWitness {
    mount_source: String,
    mount_root: String,
    mount_target: String,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    filesystem_type_magic: u64,
    filesystem_type_name: String,
    filesystem_id: u64,
    guest_boot_id: String,
    total_bytes: u64,
    free_bytes: u64,
    available_bytes: u64,
    inode_total: u64,
    inode_free: u64,
    inode_available: u64,
    inode_policy: &'static str,
}

impl ProbeStorageMountWitness {
    fn from_record(record: &ProbeRecord) -> Self {
        let inode_policy = match record.docker_root_inode_policy {
            InodePolicy::Finite => "finite",
            InodePolicy::DynamicBtrfs => "dynamic_btrfs",
            InodePolicy::Unknown => "unknown",
        };
        Self {
            mount_source: record.docker_root_mount_source.clone(),
            mount_root: record.docker_root_mount_root.clone(),
            mount_target: record.docker_root_mount_target.clone(),
            mount_id: record.docker_root_mount_id,
            device_major: record.docker_root_device_major,
            device_minor: record.docker_root_device_minor,
            statx_device_major: record.docker_root_statx_device_major,
            statx_device_minor: record.docker_root_statx_device_minor,
            filesystem_type_magic: record.docker_root_fs_type_magic,
            filesystem_type_name: record.docker_root_fs_type_name.clone(),
            filesystem_id: record.docker_root_filesystem_id,
            guest_boot_id: record.guest_boot_id.clone(),
            total_bytes: record.docker_root_total_bytes,
            free_bytes: record.docker_root_free_bytes,
            available_bytes: record.docker_root_available_bytes,
            inode_total: record.docker_root_inode_total,
            inode_free: record.docker_root_inode_free,
            inode_available: record.docker_root_inode_available,
            inode_policy,
        }
    }

    pub(crate) fn mount_source(&self) -> &str {
        &self.mount_source
    }
    pub(crate) fn mount_root(&self) -> &str {
        &self.mount_root
    }
    pub(crate) fn mount_target(&self) -> &str {
        &self.mount_target
    }
    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }
    pub(crate) const fn device_major(&self) -> u32 {
        self.device_major
    }
    pub(crate) const fn device_minor(&self) -> u32 {
        self.device_minor
    }
    pub(crate) const fn statx_device_major(&self) -> u32 {
        self.statx_device_major
    }
    pub(crate) const fn statx_device_minor(&self) -> u32 {
        self.statx_device_minor
    }
    pub(crate) const fn filesystem_type_magic(&self) -> u64 {
        self.filesystem_type_magic
    }
    pub(crate) fn filesystem_type_name(&self) -> &str {
        &self.filesystem_type_name
    }
    pub(crate) const fn filesystem_id(&self) -> u64 {
        self.filesystem_id
    }
    pub(crate) fn guest_boot_id(&self) -> &str {
        &self.guest_boot_id
    }
    pub(crate) const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
    pub(crate) const fn free_bytes(&self) -> u64 {
        self.free_bytes
    }
    pub(crate) const fn available_bytes(&self) -> u64 {
        self.available_bytes
    }
    pub(crate) const fn inode_total(&self) -> u64 {
        self.inode_total
    }
    pub(crate) const fn inode_free(&self) -> u64 {
        self.inode_free
    }
    pub(crate) const fn inode_available(&self) -> u64 {
        self.inode_available
    }
    pub(crate) const fn inode_policy(&self) -> &'static str {
        self.inode_policy
    }
}

/// Freshness evidence for a charged capacity reservation; it cannot reserve more capacity.
#[derive(Debug, Clone)]
pub(crate) struct StartObservationWitness {
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    pub(super) observed_at: Instant,
    storage_mount: ProbeStorageMountWitness,
}

impl StartObservationWitness {
    pub(super) fn from_observation(
        record: &ProbeRecord,
        engine_id: String,
        docker_root_digest: String,
        sample_generation_id: String,
        observed_at: Instant,
    ) -> Self {
        Self {
            engine_id,
            docker_root_digest,
            sample_generation_id,
            observed_at,
            storage_mount: ProbeStorageMountWitness::from_record(record),
        }
    }

    pub(crate) fn storage_mount(&self) -> &ProbeStorageMountWitness {
        &self.storage_mount
    }

    /// Whether the original sample remains fresh for this exact engine and Docker root.
    #[must_use]
    pub(crate) fn is_fresh_for(
        &self,
        engine_id: &str,
        docker_root_digest: &str,
        sample_generation_id: &str,
    ) -> bool {
        self.engine_id == engine_id
            && self.docker_root_digest == docker_root_digest
            && self.sample_generation_id == sample_generation_id
            && self.observed_at.elapsed() <= MAX_OBSERVATION_AGE
    }

    #[cfg(test)]
    pub(crate) fn test_fixture_for(
        engine_id: &str,
        docker_root_digest: &str,
        stale: bool,
    ) -> (Self, String) {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_GENERATION: AtomicU64 = AtomicU64::new(0);
        let generation = format!("{:032x}", NEXT_GENERATION.fetch_add(1, Ordering::Relaxed));
        let mut observed_at = Instant::now();
        if stale {
            observed_at -= MAX_OBSERVATION_AGE + Duration::from_millis(1);
        }
        let record = super::super::record::fixture_record();
        (
            Self::from_observation(
                &record,
                engine_id.to_owned(),
                docker_root_digest.to_owned(),
                generation.clone(),
                observed_at,
            ),
            generation,
        )
    }

    #[cfg(test)]
    fn is_fresh_at(
        &self,
        engine_id: &str,
        root_digest: &str,
        generation_id: &str,
        now: Instant,
    ) -> bool {
        self.engine_id == engine_id
            && self.docker_root_digest == root_digest
            && self.sample_generation_id == generation_id
            && now
                .checked_duration_since(self.observed_at)
                .is_some_and(|age| age <= MAX_OBSERVATION_AGE)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::super::StartPermit;

    #[test]
    fn witness_binds_mount_identity_and_sample_freshness() -> Result<(), String> {
        let permit = StartPermit::test_fixture_with_generation(&"a".repeat(32));
        let root = permit.docker_root_digest().to_owned();
        let witness = permit.observation_witness();
        assert!(witness.is_fresh_for("test-engine", &root, &"a".repeat(32)));
        assert_eq!(
            witness.storage_mount().mount_target(),
            "/velnor/docker-root"
        );
        assert_eq!(witness.storage_mount().mount_source(), "/dev/vdb");
        assert_eq!(witness.storage_mount().mount_root(), "/_data");
        assert_eq!(
            witness.storage_mount().guest_boot_id(),
            "12345678-1234-1234-1234-123456789abc"
        );
        assert!(!witness.is_fresh_for("other-engine", &root, &"a".repeat(32)));
        assert!(!witness.is_fresh_for("test-engine", &"b".repeat(64), &"a".repeat(32)));
        assert!(!witness.is_fresh_for("test-engine", &root, &"c".repeat(32)));
        let stale = witness
            .observed_at
            .checked_add(Duration::from_millis(30_001))
            .ok_or_else(|| "could not construct expiry instant".to_owned())?;
        assert!(!witness.is_fresh_at("test-engine", &root, &"a".repeat(32), stale));
        Ok(())
    }
}
