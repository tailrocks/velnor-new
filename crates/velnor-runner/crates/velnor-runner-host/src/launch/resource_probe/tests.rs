use super::projection::{DockerRoot, ProbeProjection, VerifiedProbeImage, expected_image_config};
use super::record::{InodePolicy, ProbeRecord, fixture_record};
use crate::Journal;
use crate::journal::{CapacityLimits, OperationalCapacityLimits, ProbePhase, ProbeSeed};
use crate::launch_harness::Scratch;
use bollard::models::HostConfigCgroupnsModeEnum;
use record_fixture::{GIB, MEMORY_TOTAL, record, record_values};

mod cleanup_proof;
mod docker_lifecycle;
mod probe_cpu_pid_binding;
mod projection_digest;
mod record_fixture;
mod sample_permit;

#[test]
fn parser_accepts_canonical_record_and_nullable_diagnostic() {
    assert!(ProbeRecord::parse(&record("null"), MEMORY_TOTAL).is_some());
    assert!(ProbeRecord::parse(&record("125"), MEMORY_TOTAL).is_some());
}

#[test]
fn parser_accepts_verified_btrfs_mountinfo_and_statx_device_difference() {
    let record = fixture_record();
    assert_eq!(record.docker_root_device_major, 0);
    assert_eq!(record.docker_root_device_minor, 38);
    assert_eq!(record.docker_root_statx_device_major, 0);
    assert_eq!(record.docker_root_statx_device_minor, 41);
    assert!(ProbeRecord::parse(&serialize(&record), MEMORY_TOTAL).is_some());
}

#[test]
fn parser_requires_matching_devices_for_non_btrfs_filesystems() {
    let mut ext4 = fixture_record();
    ext4.docker_root_fs_type_magic = 0xEF53;
    ext4.docker_root_fs_type_name = "ext4".to_owned();
    ext4.docker_root_inode_policy = InodePolicy::Finite;
    ext4.docker_root_inode_total = 100;
    ext4.docker_root_inode_free = 90;
    ext4.docker_root_inode_available = 80;
    assert!(ProbeRecord::parse(&serialize(&ext4), MEMORY_TOTAL).is_none());

    ext4.docker_root_statx_device_minor = ext4.docker_root_device_minor;
    assert!(ProbeRecord::parse(&serialize(&ext4), MEMORY_TOTAL).is_some());

    let mut mislabeled_btrfs = fixture_record();
    mislabeled_btrfs.docker_root_fs_type_name = "ext4".to_owned();
    assert!(ProbeRecord::parse(&serialize(&mislabeled_btrfs), MEMORY_TOTAL).is_none());
}

#[test]
fn parser_rejects_noncanonical_framing_order_and_duplicates() {
    let valid = record("null");
    let mut two_newlines = valid.clone();
    two_newlines.push(b'\n');
    let reordered = serde_json::to_vec(&serde_json::to_value(fixture_record()).unwrap_or_default())
        .unwrap_or_default();
    let duplicate = valid
        .windows(b"\"schema_version\":3".len())
        .position(|part| part == b"\"schema_version\":3")
        .map(|position| {
            let mut bytes = valid.clone();
            bytes.splice(position..position, b"\"schema_version\":3,".iter().copied());
            bytes
        })
        .unwrap_or_default();

    assert!(ProbeRecord::parse(&valid[..valid.len() - 1], MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(&two_newlines, MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(&reordered, MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(&duplicate, MEMORY_TOTAL).is_none());
}

#[test]
fn parser_rejects_bad_values_missing_fields_and_oversize_output() {
    for invalid in [
        record_values(3, 41, 40, 30, 125, "null"),
        record_values(3, 20 * GIB, 40 * GIB, MEMORY_TOTAL + 1, 125, "null"),
        record_values(3, 20 * GIB, 40 * GIB, 30, 125, "10001"),
        record_values(1, 20 * GIB, 40 * GIB, 30, 125, "null"),
        b"{}\n".to_vec(),
    ] {
        assert!(ProbeRecord::parse(&invalid, MEMORY_TOTAL).is_none());
    }
    let mut missing = fixture_record();
    missing.docker_root_mount_id = 0;
    assert!(ProbeRecord::parse(&serialize(&missing), MEMORY_TOTAL).is_none());
    let mut missing_statx_device = serde_json::to_value(fixture_record()).unwrap_or_default();
    if let Some(object) = missing_statx_device.as_object_mut() {
        object.remove("docker_root_statx_device_minor");
    }
    let mut missing_statx_device = serde_json::to_vec(&missing_statx_device).unwrap_or_default();
    missing_statx_device.push(b'\n');
    assert!(ProbeRecord::parse(&missing_statx_device, MEMORY_TOTAL).is_none());
    let mut bad_inode_policy = fixture_record();
    bad_inode_policy.docker_root_inode_policy = InodePolicy::Finite;
    assert!(ProbeRecord::parse(&serialize(&bad_inode_policy), MEMORY_TOTAL).is_none());
    let mut bad_cgroup = fixture_record();
    bad_cgroup.probe_cgroup_memory_max_bytes = Some(1);
    assert!(ProbeRecord::parse(&serialize(&bad_cgroup), MEMORY_TOTAL).is_none());
    let mut unexpected = serde_json::to_value(fixture_record()).unwrap_or_default();
    if let Some(object) = unexpected.as_object_mut() {
        object.insert("unexpected".to_owned(), serde_json::Value::from(1));
    }
    let mut unexpected = serde_json::to_vec(&unexpected).unwrap_or_default();
    unexpected.push(b'\n');
    assert!(ProbeRecord::parse(&unexpected, MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(&record("null"), 0).is_none());
    assert!(ProbeRecord::parse(&vec![b' '; 2048], MEMORY_TOTAL).is_none());
    let mut oversize = vec![b' '; 2048];
    oversize.push(b'\n');
    assert!(ProbeRecord::parse(&oversize, MEMORY_TOTAL).is_none());
}

fn serialize(record: &ProbeRecord) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(record).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

#[test]
fn docker_root_is_normalized_redacted_and_domain_separated() {
    let first = DockerRoot::parse("/var/lib/docker");
    let second = DockerRoot::parse("/var/lib/containerd");
    if let (Ok(first), Ok(second)) = (first, second) {
        assert_ne!(first.digest(), second.digest());
        assert!(!format!("{first:?}").contains("/var/lib/docker"));
    }
    for invalid in ["", ".", "/", "relative/path", "/a/../b", "/a//b", "/a/"] {
        assert!(DockerRoot::parse(invalid).is_err());
    }
}

#[test]
fn projection_is_private_and_uses_only_the_fixed_guest_mount() {
    let projection = ProbeProjection::build(
        "d".repeat(32),
        "e".repeat(32),
        "selected-engine".to_owned(),
        "/var/lib/docker",
        VerifiedProbeImage::test_fixture(),
    );
    assert!(projection.is_ok());
    if let Ok(projection) = projection {
        assert_eq!(
            projection.options.name.as_deref(),
            Some(projection.name.as_str())
        );
        assert_eq!(projection.config.user.as_deref(), Some("65532:65532"));
        assert!(projection.config.env.is_none());
        assert!(projection.config.cmd.is_none());
        assert!(projection.config.entrypoint.is_none());
        let host = projection.config.host_config.as_ref();
        if let Some(host) = host {
            assert_eq!(host.network_mode.as_deref(), Some("none"));
            assert_eq!(host.readonly_rootfs, Some(true));
            assert_eq!(host.privileged, Some(false));
            assert_eq!(host.cpu_period, Some(100_000));
            assert_eq!(host.cpu_quota, Some(10_000));
            assert_eq!(host.memory, Some(128 * 1024 * 1024));
            assert_eq!(host.memory_swap, Some(128 * 1024 * 1024));
            assert_eq!(host.pids_limit, Some(4));
            assert_eq!(host.cap_drop.as_ref().map(Vec::len), Some(1));
            assert_eq!(
                host.cap_drop
                    .as_ref()
                    .and_then(|values| values.first())
                    .map(String::as_str),
                Some("ALL")
            );
            assert_eq!(host.mounts.as_ref().map(Vec::len), Some(2));
            assert_eq!(host.pid_mode.as_deref(), Some("host"));
            assert_eq!(host.cgroupns_mode, Some(HostConfigCgroupnsModeEnum::HOST));
            if let Some(mounts) = &host.mounts {
                assert_eq!(mounts[0].source.as_deref(), Some("/var/lib/docker"));
                assert_eq!(mounts[1].source.as_deref(), Some("/sys/fs/cgroup"));
                assert!(mounts.iter().all(|mount| mount.read_only == Some(true)));
            }
        }
        assert!(!format!("{projection:?}").contains("/var/lib/docker"));
        assert_eq!(projection.root.digest().len(), 64);
    }
}

#[test]
fn verified_image_requires_exact_source_config_and_immutable_platform_id() {
    let revision = "b".repeat(40);
    let fingerprint = "c".repeat(64);
    let id = format!("sha256:{}", "a".repeat(64));
    let config = expected_image_config(&revision);
    let valid = VerifiedProbeImage::from_verified_provider(
        id.clone(),
        "linux/amd64".to_owned(),
        "x86_64".to_owned(),
        revision.clone(),
        fingerprint.clone(),
        &config,
    );
    assert!(valid.is_ok());
    assert!(
        VerifiedProbeImage::from_verified_provider(
            "velnor/resource-probe:latest".to_owned(),
            "linux/amd64".to_owned(),
            "x86_64".to_owned(),
            revision.clone(),
            fingerprint.clone(),
            &config,
        )
        .is_err()
    );
    assert!(
        VerifiedProbeImage::from_verified_provider(
            id,
            "linux/arm64".to_owned(),
            "x86_64".to_owned(),
            revision.clone(),
            fingerprint,
            &config,
        )
        .is_err()
    );
    let mut untrusted = config;
    untrusted.cmd = Some(vec!["sh".to_owned()]);
    assert!(
        VerifiedProbeImage::from_verified_provider(
            format!("sha256:{}", "a".repeat(64)),
            "linux/amd64".to_owned(),
            "x86_64".to_owned(),
            revision,
            "c".repeat(64),
            &untrusted,
        )
        .is_err()
    );
}

#[tokio::test]
async fn journaled_probe_phases_are_exact_and_prepared_abort_is_effect_free() -> Result<(), String>
{
    let scratch = Scratch::new("probe-journal").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let launch = journal
        .begin("launch", "probe-test-launch")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("probe-engine")
        .await
        .map_err(|error| error.to_string())?;
    configure_probe_capacity(&journal).await?;
    let seed = probe_seed(&journal).await?;
    journal
        .prepare_probe(seed)
        .await
        .map_err(|error| error.to_string())?;
    let prepared = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        prepared.as_ref().map(|row| row.phase),
        Some(ProbePhase::Prepared)
    );
    assert!(
        journal
            .transition_probe(&"d".repeat(32), ProbePhase::Prepared, ProbePhase::Started)
            .await
            .is_err()
    );
    journal
        .abort_prepared_probe(&"d".repeat(32))
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, launch);
    let bytes = std::fs::read(scratch.file()).map_err(|error| error.to_string())?;
    assert!(
        !bytes
            .windows(b"/var/lib/docker".len())
            .any(|window| window == b"/var/lib/docker")
    );
    Ok(())
}

#[tokio::test]
async fn probe_transition_requires_intents_and_exact_container_id() -> Result<(), String> {
    let scratch = Scratch::new("probe-transition").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_engine("probe-engine")
        .await
        .map_err(|error| error.to_string())?;
    configure_probe_capacity(&journal).await?;
    journal
        .prepare_probe(probe_seed(&journal).await?)
        .await
        .map_err(|error| error.to_string())?;
    let operation = "d".repeat(32);
    journal
        .transition_probe(
            &operation,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .bind_probe_container(&operation, "short-id")
            .await
            .is_err()
    );
    journal
        .bind_probe_container(&operation, &"a".repeat(64))
        .await
        .map_err(|error| error.to_string())?;
    let row = journal
        .active_probe()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "probe row missing".to_owned())?;
    assert_eq!(row.phase, ProbePhase::ContainerCreated);
    assert_eq!(row.container_id.as_deref(), Some("a".repeat(64).as_str()));
    journal
        .quarantine_probe(&operation)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .active_probe()
            .await
            .map_err(|error| error.to_string())?
            .map(|row| row.phase),
        Some(ProbePhase::Quarantined)
    );
    assert!(
        journal
            .prepare_probe(probe_seed_with_id(&journal, "f").await?)
            .await
            .is_err()
    );
    Ok(())
}

pub(super) async fn configure_probe_capacity(journal: &Journal) -> Result<(), String> {
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 8_589_934_592,
            },
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 268_435_456,
            },
        )
        .await
        .map_err(|error| error.to_string())
}

async fn probe_seed(journal: &Journal) -> Result<ProbeSeed, String> {
    probe_seed_with_id(journal, "d").await
}

async fn probe_seed_with_id(journal: &Journal, nibble: &str) -> Result<ProbeSeed, String> {
    let operation_id = nibble.repeat(32);
    let instance_id = journal
        .instance_id()
        .await
        .map_err(|error| error.to_string())?;
    Ok(ProbeSeed {
        operation_name: format!("velnor-resource-probe-{operation_id}"),
        operation_id,
        instance_id,
        engine_id: "probe-engine".to_owned(),
        docker_root_digest: "a".repeat(64),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "e".repeat(64),
        projection_digest: "f".repeat(64),
    })
}
