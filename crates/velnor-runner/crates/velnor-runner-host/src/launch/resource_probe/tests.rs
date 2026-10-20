use super::projection::{DockerRoot, ProbeProjection, VerifiedProbeImage, expected_image_config};
use super::record::{InodePolicy, ProbeRecord, fixture_record};
use crate::Journal;
use crate::journal::{
    CapacityLimits, OperationalCapacityLimits, ProbePhase, ProbeSeed, ResourceProbePurpose,
};
use crate::launch_harness::Scratch;
use bollard::models::HostConfigCgroupnsModeEnum;
use record_fixture::{GIB, MEMORY_TOTAL, record, record_values};

mod cleanup_proof;
mod docker_lifecycle;
mod probe_cpu_pid_binding;
mod projection_digest;
mod record_fixture;
#[path = "tests/record_parsing.rs"]
mod record_parsing;
mod sample_permit;

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
        purpose: ResourceProbePurpose::Capacity,
        request_sha256: crate::journal::capacity_request_sha256(),
    })
}
