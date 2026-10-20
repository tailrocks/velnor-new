use super::{OwnedCgroupSubjects, OwnedContainerSubjects, parse_record};
use crate::launch::resource_probe::projection::{
    ProbeProjection, VerifiedProbeImage, expected_image_config,
};
use sha2::{Digest, Sha256};
use velnor_resource_probe::worker_binding::{OwnedCgroupObservation, OwnedCgroupRequest};

fn response_fixture(
    schema_version: u8,
) -> Result<(OwnedCgroupRequest, OwnedCgroupObservation, Vec<u8>), serde_json::Error> {
    let request = OwnedCgroupRequest {
        schema_version: 2,
        operation_id: "c".repeat(32),
        runner_container_id: "a".repeat(64),
        runner_init_pid: 100,
        runner_init_start_ticks: 1_000,
        runner_helper_pid: 101,
        runner_helper_start_ticks: 1_001,
        dind_container_id: "b".repeat(64),
        dind_init_pid: 200,
        dind_init_start_ticks: 2_000,
        dind_helper_pid: 201,
        dind_helper_start_ticks: 2_001,
        docker_root_path: "/var/lib/docker".to_owned(),
    };
    let runner_init = process(100, 1_000, 501, "/runner/init");
    let runner_helper = process(101, 1_001, 501, "/runner/helper");
    let dind_init = process(200, 2_000, 502, "/dind/init");
    let dind_helper = process(201, 2_001, 502, "/dind/helper");
    let value = response_value(
        schema_version,
        &request,
        runner_init,
        runner_helper,
        dind_init,
        dind_helper,
    );
    let observation: OwnedCgroupObservation = serde_json::from_value(value)?;
    let mut bytes = serde_json::to_vec(&observation)?;
    bytes.push(b'\n');
    Ok((request, observation, bytes))
}

fn response_value(
    schema_version: u8,
    request: &OwnedCgroupRequest,
    runner_init: serde_json::Value,
    runner_helper: serde_json::Value,
    dind_init: serde_json::Value,
    dind_helper: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "wire_mode": "owned_container_cgroup",
        "schema_version": schema_version,
        "operation_id": request.operation_id.clone(),
        "guest_boot_id": "12345678-1234-1234-1234-123456789abc",
        "engine_metadata": {
            "role": "engine_metadata",
            "target": "/velnor/docker-root",
            "read_only": true,
            "executable_mount": false,
            "mount_id": 10,
            "statx_mount_id": 10,
            "statx_inode": 30,
            "mount_root": "/",
            "mount_source": "/var/lib/docker",
            "mount_device_major": 0,
            "mount_device_minor": 38,
            "statx_device_major": 0,
            "statx_device_minor": 38,
            "filesystem_type_magic": 2_435_016_766_u64,
            "filesystem_type_name": "btrfs",
            "filesystem_id": 17,
            "block_size": 4096,
            "total_bytes": 10000,
            "free_bytes": 5000,
            "available_bytes": 4000,
            "inode_total": 0,
            "inode_free": 0,
            "inode_available": 0,
            "inode_policy": "dynamic_btrfs"
        },
        "docker_root_object": {
            "mount_id": 10,
            "device_major": 0,
            "device_minor": 38,
            "inode": 30,
            "filesystem_type_magic": 2_435_016_766_u64,
            "filesystem_id": 17
        },
        "daemon": process(50, 500, 500, "/system/docker"),
        "containers": [
            {
                "role": "runner",
                "container_id": request.runner_container_id.clone(),
                "init": runner_init,
                "helper": runner_helper,
                "hierarchy": [cgroup_node("/runner", true)]
            },
            {
                "role": "dind",
                "container_id": request.dind_container_id.clone(),
                "init": dind_init,
                "helper": dind_helper,
                "hierarchy": [cgroup_node("/dind", true)]
            }
        ],
        "root_backings": [
            root_backing("runner", &request.runner_container_id, "12345678-1234-1234-1234-123456789abc", runner_init, runner_helper),
            root_backing("dind", &request.dind_container_id, "12345678-1234-1234-1234-123456789abc", dind_init, dind_helper)
        ]
    })
}

fn process(host_pid: u32, start_ticks: u64, namespace_id: u64, path: &str) -> serde_json::Value {
    serde_json::json!({
        "host_pid": host_pid,
        "start_ticks": start_ticks,
        "mount_namespace_id": namespace_id,
        "mount_namespace_device_major": 0,
        "mount_namespace_device_minor": namespace_id,
        "namespace_pids": [host_pid],
        "cgroup_path": path
    })
}

fn cgroup_node(path: &str, root: bool) -> serde_json::Value {
    serde_json::json!({
        "path": path,
        "root": root,
        "memory_max_bytes": null,
        "memory_current_bytes": 1,
        "swap_max_bytes": null,
        "swap_current_bytes": 0,
        "cpu_quota_micros": null,
        "cpu_period_micros": 100_000,
        "cpuset_cpus": [0],
        "pids_max": null,
        "pids_current": 1
    })
}

fn root_backing(
    role: &str,
    container_id: &str,
    boot_id: &str,
    init: serde_json::Value,
    helper: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "role": role,
        "container_id": container_id,
        "guest_boot_id": boot_id,
        "init": init,
        "helper": helper,
        "mount_source": "/var/lib/docker/overlay2/worker/merged",
        "filesystem_type": "overlay",
        "mount_id": 20,
        "mount_device_major": 0,
        "mount_device_minor": 38,
        "mount_root": "/",
        "read_only": false,
        "upperdir": "/var/lib/docker/overlay2/worker/diff",
        "workdir": "/var/lib/docker/overlay2/worker/work",
        "lowerdirs": ["/var/lib/docker/overlay2/lower/diff"],
        "directories": []
    })
}

#[test]
fn response_schema_roundtrips_through_the_host_parser() -> Result<(), serde_json::Error> {
    let (request, observation, bytes) = response_fixture(OwnedCgroupObservation::SCHEMA_VERSION)?;

    assert_eq!(
        parse_record(&bytes, &request).as_ref().ok(),
        Some(&observation)
    );
    Ok(())
}

#[test]
fn response_parser_rejects_old_and_unknown_schema_versions() -> Result<(), serde_json::Error> {
    for version in [2, OwnedCgroupObservation::SCHEMA_VERSION + 1] {
        let (request, _, bytes) = response_fixture(version)?;
        assert!(matches!(
            parse_record(&bytes, &request),
            Err(crate::error::HostError::Identity)
        ));
    }
    Ok(())
}

fn subject(
    id: char,
    init: u32,
    helper: u32,
) -> Result<OwnedContainerSubjects, crate::error::HostError> {
    OwnedContainerSubjects::new(id.to_string().repeat(64), init, 1, helper, 2)
}

#[test]
fn container_subjects_reject_invalid_identity() {
    assert!(OwnedContainerSubjects::new("short".to_owned(), 1, 1, 2, 2).is_err());
    assert!(OwnedContainerSubjects::new("a".repeat(64), 1, 1, 1, 2).is_err());
    assert!(OwnedContainerSubjects::new("a".repeat(64), 1, 0, 2, 2).is_err());
}

#[test]
fn pair_subjects_reject_duplicate_containers_and_processes() {
    let runner = subject('a', 10, 11);
    let same_container = subject('a', 12, 13);
    let same_process = subject('b', 10, 13);
    assert!(runner.is_ok());
    assert!(same_container.is_ok());
    assert!(same_process.is_ok());
    if let (Ok(runner), Ok(dind)) = (runner, same_container) {
        assert!(OwnedCgroupSubjects::new(runner, dind).is_err());
    }
    if let (Ok(runner), Ok(dind)) = (subject('a', 10, 11), same_process) {
        assert!(OwnedCgroupSubjects::new(runner, dind).is_err());
    }
}

#[test]
fn owned_request_is_one_canonical_bounded_line() {
    let runner = subject('a', 10, 11);
    let dind = subject('b', 20, 21);
    assert!(runner.is_ok());
    assert!(dind.is_ok());
    if let (Ok(runner), Ok(dind)) = (runner, dind)
        && let Ok(subjects) = OwnedCgroupSubjects::new(runner, dind)
        && let Ok(line) = subjects.request_line(&"c".repeat(32), "/var/lib/docker")
    {
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(line.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert!(line.len() <= velnor_resource_probe::worker_binding::WORKER_REQUEST_LIMIT);
        assert!(
            serde_json::from_slice::<velnor_resource_probe::worker_binding::OwnedCgroupRequest>(
                &line[..line.len() - 1]
            )
            .is_ok()
        );
        let expected = Sha256::digest(
            [
                b"velnor-owned-cgroups-request-v1\0".as_slice(),
                line.as_slice(),
            ]
            .concat(),
        );
        assert_eq!(
            subjects.request_sha256(&"c".repeat(32), "/var/lib/docker"),
            Ok(expected
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>())
        );
    }
}

#[test]
fn owned_projection_uses_fixed_main_mode_and_private_host_pid_access() {
    let revision = "b".repeat(40);
    let image = VerifiedProbeImage::from_verified_provider(
        format!("sha256:{}", "a".repeat(64)),
        "linux/amd64".to_owned(),
        "aarch64".to_owned(),
        revision.clone(),
        "c".repeat(64),
        &expected_image_config(&revision),
    );
    assert!(image.is_ok());
    if let Ok(image) = image {
        let projection = ProbeProjection::build_owned_cgroups(
            "d".repeat(32),
            "e".repeat(32),
            "selected-engine".to_owned(),
            "/var/lib/docker",
            image,
            "f".repeat(64),
        );
        assert!(projection.is_ok());
        if let Ok(projection) = projection {
            assert_eq!(
                projection.config.cmd.as_deref(),
                Some(["--mode".to_owned(), "owned_container_cgroup".to_owned()].as_slice())
            );
            assert_eq!(projection.config.attach_stdin, Some(true));
            assert_eq!(projection.config.open_stdin, Some(true));
            assert_eq!(projection.config.stdin_once, Some(true));
            let host = projection.config.host_config.as_ref();
            assert!(host.is_some_and(|config| {
                config.pid_mode.as_deref() == Some("host")
                    && config.privileged == Some(false)
                    && config.network_mode.as_deref() == Some("none")
                    && config.readonly_rootfs == Some(true)
            }));
        }
    }
}
