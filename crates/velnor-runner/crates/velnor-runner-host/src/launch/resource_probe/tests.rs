use super::projection::{DockerRoot, ProbeProjection, VerifiedProbeImage, expected_image_config};
use super::record::ProbeRecord;
use super::sample::Observation;
use crate::Journal;
use crate::journal::{ProbePhase, ProbeSeed};
use crate::launch_harness::Scratch;

const MEMORY_TOTAL: u64 = 8 * 1024 * 1024 * 1024;

fn record(psi: &str) -> Vec<u8> {
    record_values(1, 20, 40, 30, 125, psi)
}

fn record_values(version: u8, free: u64, total: u64, memory: u64, load: u64, psi: &str) -> Vec<u8> {
    format!(
        "{{\"schema_version\":{version},\"docker_root_free_bytes\":{free},\"docker_root_total_bytes\":{total},\"memory_available_bytes\":{memory},\"load_milli\":{load},\"memory_psi_some_avg10_bps\":{psi}}}\n"
    )
    .into_bytes()
}

#[test]
fn parser_accepts_canonical_record_and_nullable_diagnostic() {
    assert!(ProbeRecord::parse(&record("null"), MEMORY_TOTAL).is_some());
    assert!(ProbeRecord::parse(&record("125"), MEMORY_TOTAL).is_some());
}

#[test]
fn parser_rejects_noncanonical_framing_order_and_duplicates() {
    let valid = record("null");
    let mut two_newlines = valid.clone();
    two_newlines.push(b'\n');
    let reordered = b"{\"docker_root_free_bytes\":20,\"schema_version\":1,\"docker_root_total_bytes\":40,\"memory_available_bytes\":30,\"load_milli\":125,\"memory_psi_some_avg10_bps\":null}\n";
    let duplicate = b"{\"schema_version\":1,\"schema_version\":1,\"docker_root_free_bytes\":20,\"docker_root_total_bytes\":40,\"memory_available_bytes\":30,\"load_milli\":125,\"memory_psi_some_avg10_bps\":null}\n";

    assert!(ProbeRecord::parse(&valid[..valid.len() - 1], MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(&two_newlines, MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(reordered, MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(duplicate, MEMORY_TOTAL).is_none());
}

#[test]
fn parser_rejects_bad_values_missing_fields_and_oversize_output() {
    for invalid in [
        record_values(1, 41, 40, 30, 125, "null"),
        record_values(1, 20, 40, MEMORY_TOTAL + 1, 125, "null"),
        record_values(1, 20, 40, 30, 125, "10001"),
        record_values(2, 20, 40, 30, 125, "null"),
        record_values(1, 20, 40, 30, u64::from(u32::MAX) + 1, "null"),
        b"{\"schema_version\":1}\n".to_vec(),
        b"{\"schema_version\":1,\"docker_root_free_bytes\":20,\"docker_root_total_bytes\":40,\"memory_available_bytes\":30,\"load_milli\":125,\"memory_psi_some_avg10_bps\":null,\"unexpected\":1}\n".to_vec(),
        b"{}\n".to_vec(),
    ] {
        assert!(ProbeRecord::parse(&invalid, MEMORY_TOTAL).is_none());
    }
    assert!(ProbeRecord::parse(&record("null"), 0).is_none());
    assert!(ProbeRecord::parse(&vec![b' '; 512], MEMORY_TOTAL).is_none());
    let mut oversize = vec![b' '; 512];
    oversize.push(b'\n');
    assert!(ProbeRecord::parse(&oversize, MEMORY_TOTAL).is_none());
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
            assert_eq!(host.cap_drop.as_ref().map(Vec::len), Some(1));
            assert_eq!(
                host.cap_drop
                    .as_ref()
                    .and_then(|values| values.first())
                    .map(String::as_str),
                Some("ALL")
            );
            assert_eq!(host.mounts.as_ref().map(Vec::len), Some(1));
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
        revision.clone(),
        fingerprint.clone(),
        &config,
    );
    assert!(valid.is_ok());
    assert!(
        VerifiedProbeImage::from_verified_provider(
            "velnor/resource-probe:latest".to_owned(),
            "linux/amd64".to_owned(),
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

#[test]
fn sample_permit_requires_recent_bounded_metrics_and_matching_engine() -> Result<(), String> {
    use std::time::{Duration, Instant};

    use crate::worker::test_resource_budget;

    let budget = test_resource_budget().map_err(|error| error.to_string())?;
    let pair = budget.pair();
    let mem_total = pair.memory_bytes + 1;
    let root_digest = "a".repeat(64);
    let sample_record = ProbeRecord {
        schema_version: 1,
        docker_root_free_bytes: 20 * 1024 * 1024 * 1024,
        docker_root_total_bytes: 40 * 1024 * 1024 * 1024,
        memory_available_bytes: pair.memory_bytes,
        load_milli: 125,
        memory_psi_some_avg10_bps: None,
    };
    let sample = Observation::new(
        sample_record.clone(),
        8,
        mem_total,
        "selected-engine".to_owned(),
        root_digest.clone(),
        Instant::now(),
    )
    .ok_or_else(|| "valid guest record was rejected".to_owned())?;
    let permit = sample
        .into_start_permit(budget)
        .ok_or_else(|| "valid fresh sample did not yield a permit".to_owned())?;
    assert!(super::consume_permit(
        permit,
        "selected-engine",
        &root_digest
    ));

    let stale_at = Instant::now()
        .checked_sub(Duration::from_secs(31))
        .ok_or_else(|| "could not construct stale instant".to_owned())?;
    let stale = Observation::new(
        sample_record.clone(),
        8,
        mem_total,
        "selected-engine".to_owned(),
        root_digest.clone(),
        stale_at,
    )
    .ok_or_else(|| "stale sample record was rejected too early".to_owned())?;
    assert!(stale.into_start_permit(budget).is_none());

    let mut low_disk = sample_record.clone();
    low_disk.docker_root_free_bytes = 10 * 1024 * 1024 * 1024 - 1;
    let low_disk = Observation::new(
        low_disk,
        8,
        mem_total,
        "selected-engine".to_owned(),
        root_digest.clone(),
        Instant::now(),
    )
    .ok_or_else(|| "low disk record was rejected before admission".to_owned())?;
    assert!(low_disk.into_start_permit(budget).is_none());

    let wrong_engine = Observation::new(
        sample_record,
        8,
        mem_total,
        "selected-engine".to_owned(),
        root_digest.clone(),
        Instant::now(),
    )
    .ok_or_else(|| "valid sample record was rejected".to_owned())?
    .into_start_permit(budget)
    .ok_or_else(|| "valid fresh sample did not yield a permit".to_owned())?;
    assert!(!super::consume_permit(
        wrong_engine,
        "other-engine",
        &root_digest
    ));
    Ok(())
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
