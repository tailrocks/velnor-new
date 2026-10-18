use std::error::Error;

use crate::capacity::docker_root_capacity;
use crate::load::one_minute_milli;
use crate::memory::available_bytes;
use crate::pressure::memory_some_avg10;
use crate::record::{CpuAncestorLimit, MAX_OUTPUT_BYTES, ProbeRecord};
use crate::units::{decimal_milli_ceil, percent_basis_points};

#[test]
fn docker_root_capacity_uses_checked_fragment_bytes_and_bounds_free_space()
-> Result<(), Box<dyn Error>> {
    assert_eq!(
        docker_root_capacity(10, 6, 4, 4096)?,
        (24_576, 40_960, 16_384)
    );
    assert_eq!(docker_root_capacity(0, 0, 0, 4096)?, (0, 0, 0));
    assert!(docker_root_capacity(u64::MAX, 0, 0, 2).is_err());
    assert!(docker_root_capacity(10, u64::MAX, 0, 2).is_err());
    assert!(docker_root_capacity(10, 4, 5, 1).is_err());
    assert!(docker_root_capacity(10, 4, 0, 0).is_err());
    Ok(())
}

#[test]
fn decimal_load_uses_checked_thousandth_ceiling() -> Result<(), Box<dyn Error>> {
    assert_eq!(decimal_milli_ceil("0")?, 0);
    assert_eq!(decimal_milli_ceil("0.000")?, 0);
    assert_eq!(decimal_milli_ceil("0.0001")?, 1);
    assert_eq!(decimal_milli_ceil("3.125")?, 3125);
    assert_eq!(decimal_milli_ceil("3.1251")?, 3126);
    assert!(decimal_milli_ceil("18446744073709552").is_err());
    for invalid in ["", ".1", "1.", "+1", "-1", "1e2", "NaN"] {
        assert!(decimal_milli_ceil(invalid).is_err());
    }
    Ok(())
}

#[test]
fn loadavg_requires_the_documented_five_fields() -> Result<(), Box<dyn Error>> {
    assert_eq!(one_minute_milli(b"1.0001 0.50 0.25 1/90 1234\n")?, 1001);
    assert!(one_minute_milli(b"0.1 0.2 0.3 1/9\n").is_err());
    assert!(one_minute_milli(b"0.1 0.2 0.3 1/9 5 extra\n").is_err());
    assert!(one_minute_milli(b"-0.1 0.2 0.3 1/9 5\n").is_err());
    Ok(())
}

#[test]
fn memavailable_is_unique_checked_and_converted_from_kib() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        available_bytes(b"MemTotal: 10 kB\nMemAvailable: 4 kB\n")?,
        4096
    );
    assert_eq!(available_bytes(b"MemAvailable: 0 kB\n")?, 0);
    assert!(available_bytes(b"MemAvailable: 18446744073709551615 kB\n").is_err());
    assert!(available_bytes(b"MemAvailable: +2 kB\n").is_err());
    assert!(available_bytes(b"MemAvailable: 2 kB extra\n").is_err());
    assert!(available_bytes(b"MemAvailable: 2 kB\nMemAvailable: 3 kB\n").is_err());
    assert!(available_bytes(b"MemTotal: 3 kB\n").is_err());
    Ok(())
}

#[test]
fn psi_percent_converts_exactly_and_rejects_unrepresentable_values() -> Result<(), Box<dyn Error>> {
    assert_eq!(percent_basis_points("0")?, 0);
    assert_eq!(percent_basis_points("0.05")?, 5);
    assert_eq!(percent_basis_points("100.00")?, 10_000);
    assert!(percent_basis_points("100.01").is_err());
    assert!(percent_basis_points("0.001").is_err());
    assert!(percent_basis_points("1e1").is_err());
    Ok(())
}

#[test]
fn psi_is_optional_but_valid_some_line_is_strict() {
    let valid = b"some avg10=0.05 avg60=0.10 avg300=0.20 total=99\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
    assert_eq!(memory_some_avg10(valid), Some(5));
    assert_eq!(memory_some_avg10(b""), None);
    assert_eq!(memory_some_avg10(b"unsupported\n"), None);
    assert_eq!(
        memory_some_avg10(b"some avg10=101.00 avg60=0.10 avg300=0.20 total=99\n"),
        None
    );
    assert_eq!(
        memory_some_avg10(b"some avg10=0.10 avg10=0.20 avg60=0.1 avg300=0.2 total=1\n"),
        None
    );
}

#[test]
fn record_json_has_stable_required_keys_and_nullable_psi() -> Result<(), Box<dyn Error>> {
    let record = ProbeRecord {
        schema_version: 3,
        docker_root_mount_target: "/velnor/docker-root".into(),
        docker_root_mount_id: 1,
        docker_root_device_major: 0,
        docker_root_device_minor: 38,
        docker_root_statx_device_major: 0,
        docker_root_statx_device_minor: 41,
        docker_root_fs_type_magic: 0x9123_683e,
        docker_root_fs_type_name: "btrfs".into(),
        docker_root_filesystem_id: 2,
        guest_boot_id: "12345678-1234-1234-1234-123456789abc".into(),
        docker_root_free_bytes: 0,
        docker_root_total_bytes: 4,
        docker_root_available_bytes: 0,
        docker_root_inode_total: 0,
        docker_root_inode_free: 0,
        docker_root_inode_available: 0,
        docker_root_inode_policy: crate::InodePolicy::DynamicBtrfs,
        memory_available_bytes: 2,
        load_milli: 3,
        memory_psi_some_avg10_bps: None,
        probe_cgroup_path: "/probe".into(),
        probe_cgroup_memory_max_bytes: None,
        probe_cgroup_memory_current_bytes: 0,
        probe_cgroup_swap_max_bytes: None,
        probe_cgroup_swap_current_bytes: 0,
        probe_cgroup_cpu_quota_micros: None,
        probe_cgroup_cpu_period_micros: 100_000,
        probe_cgroup_cpuset_cpus_count: 8,
        cpu_ancestor_limits: vec![CpuAncestorLimit {
            path: "/".into(),
            cpuset_cpus_count: 8,
            cpu_quota_micros: None,
            cpu_period_micros: 100_000,
        }],
        probe_host_pid: 1234,
        probe_namespace_pid: 1234,
        probe_cgroup_pids_max: None,
    };
    let bytes = serde_json::to_vec(&record)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let object = value
        .as_object()
        .ok_or("the probe record is not an object")?;
    assert_eq!(object.len(), 33);
    assert_eq!(value["schema_version"], 3);
    assert_eq!(value["docker_root_device_major"], 0);
    assert_eq!(value["docker_root_device_minor"], 38);
    assert_eq!(value["docker_root_statx_device_major"], 0);
    assert_eq!(value["docker_root_statx_device_minor"], 41);
    assert_eq!(value["docker_root_inode_policy"], "dynamic_btrfs");
    assert_eq!(
        value["probe_cgroup_memory_max_bytes"],
        serde_json::Value::Null
    );
    assert!(value.get("probe_cgroup_path").is_some());
    assert_eq!(value["probe_host_pid"], 1234);
    assert_eq!(value["cpu_ancestor_limits"][0]["path"], "/");
    assert!(bytes.len() < MAX_OUTPUT_BYTES);
    Ok(())
}
