use std::time::{Duration, Instant};

use bollard::models::SystemInfo;

use crate::journal::EngineContext;
use crate::launch::SelectedEngineContext;

use super::Observation;

const MEMORY_TOTAL: u64 = 16 * 1024 * 1024 * 1024;

fn selected(root: &str) -> Result<SelectedEngineContext, String> {
    let binding = EngineContext {
        engine_id: "selected-engine".to_owned(),
        endpoint: "unix:///var/run/docker.sock".to_owned(),
        platform: "linux/amd64".to_owned(),
    };
    let info = SystemInfo {
        id: Some("selected-engine".to_owned()),
        architecture: Some("aarch64".to_owned()),
        docker_root_dir: Some(root.to_owned()),
        os_type: Some("linux".to_owned()),
        ..SystemInfo::default()
    };
    SelectedEngineContext::capture(&binding, &info).map_err(|error| error.to_string())
}

fn observation() -> Result<Observation, String> {
    let root = super::super::projection::DockerRoot::parse("/var/lib/docker")
        .map_err(|error| error.to_string())?;
    Observation::new(
        super::super::record::fixture_record(),
        8,
        MEMORY_TOTAL,
        "selected-engine".to_owned(),
        root.digest().to_owned(),
        "a".repeat(32),
        Instant::now(),
    )
    .ok_or_else(|| "valid observation fixture was rejected".to_owned())
}

#[test]
fn doctor_report_contains_a_fresh_sample_for_the_selected_engine() -> Result<(), String> {
    let report = observation()?
        .doctor_report(&selected("/var/lib/docker")?)
        .ok_or_else(|| "matching fresh probe did not produce a report".to_owned())?;
    if report.sample_generation_id != "a".repeat(32)
        || report.guest_cpus != 8
        || report.memory_total_bytes != MEMORY_TOTAL
        || report.docker_root_fs_type_name != "btrfs"
        || report.docker_root_inode_policy
            != crate::readiness::ResourceProbeInodePolicy::DynamicBtrfs
    {
        return Err("probe report omitted or changed measured fields".to_owned());
    }
    Ok(())
}

#[test]
fn doctor_report_rejects_another_root_and_an_expired_observation() -> Result<(), String> {
    if observation()?
        .doctor_report(&selected("/tmp/other-docker")?)
        .is_some()
    {
        return Err("probe report was accepted for a different Docker root".to_owned());
    }
    let mut expired = observation()?;
    expired.observed_at -= Duration::from_secs(31);
    if expired
        .doctor_report(&selected("/var/lib/docker")?)
        .is_some()
    {
        return Err("expired observation produced a doctor report".to_owned());
    }
    Ok(())
}
