// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::{ContainerConfig, ContainerInspectResponse, ImageConfig};
use velnor_resource_probe::{
    cleanup_observer_wire::{
        CleanupDockerRootIdentity, CleanupObserverBinding, CleanupObserverRecord,
        CleanupObserverRequest, CleanupVolume, NamespaceIdentity, ProcessIdentity,
        docker_root_matches, validate_binding,
    },
    execution_channel_wire::ExecutionChannelObservation,
};
use velnor_runner_host::{
    BootstrapContainerRole, HostConfig, HostError, ProofChannelBindingInput,
    VerifiedProofChannelBinding, build_bootstrap_pair_create_spec, cleanup_observer_request_line,
    parse_cleanup_observer_record, validate_cleanup_observer_record, verify_pre_jit_created,
};

use crate::{TestResult, directory_and_mount};

fn sample_cleanup_binding(
    filesystem_type_magic: u64,
    mount_device: [u32; 2],
    statx_device: [u32; 2],
) -> CleanupObserverBinding {
    let proc_id = |pid, ns_ino| ProcessIdentity {
        pid,
        start_ticks: 100,
        namespace_inode: ns_ino,
        namespace_device: [0, 4],
    };
    let vol = |role: &str, inode| CleanupVolume {
        role: role.to_owned(),
        mountpoint: format!("/var/lib/docker/volumes/{role}/_data"),
        mount_device,
        statx_device,
        inode,
    };
    CleanupObserverBinding {
        expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        metadata_job_facts_inode: Some(500),
        docker_root: CleanupDockerRootIdentity {
            target: "/velnor/docker-root".to_owned(),
            mount_root: "/docker".to_owned(),
            mount_id: 90,
            statx_mount_id: 90,
            mount_device,
            statx_device,
            filesystem_type_magic,
            filesystem_id: 77,
            read_only: true,
        },
        daemon: proc_id(10, 4001),
        runner: proc_id(20, 4002),
        dind: proc_id(30, 4003),
        cgroup_paths: [
            "/docker/1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
            "/docker/2222222222222222222222222222222222222222222222222222222222222222".to_owned(),
        ],
        volume_mountpoints: vec![
            vol("shared-run", 101),
            vol("runner-home", 102),
            vol("runner-work", 103),
            vol("dind-data", 104),
            vol("engine-metadata", 105),
        ],
        owned_tmpfs_devices: [
            [0, 51],
            [0, 52],
            [0, 53],
            [0, 54],
            [0, 55],
            [0, 56],
            [0, 57],
            [0, 58],
        ],
    }
}

fn build_inbox_binding(
    fs: (&str, u64),
    mount_source: &str,
    statx_minor: u32,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let (control_dir, control_mount) = directory_and_mount(
        "/opt/velnor/source-proof/control",
        "source-proof/control",
        101,
        11,
        true,
        true,
        ("btrfs", 0x9123_683e),
    );
    let (mut inbox_dir, mut inbox_mount) = directory_and_mount(
        "/opt/velnor/source-proof/inbox",
        "source-proof/inbox",
        202,
        22,
        false,
        true,
        fs,
    );
    mount_source.clone_into(&mut inbox_dir.mount_source);
    inbox_dir.mount_device_major = 0;
    inbox_dir.mount_device_minor = 38;
    inbox_dir.statx_device_major = 0;
    inbox_dir.statx_device_minor = statx_minor;
    inbox_mount.device_major = 0;
    inbox_mount.device_minor = 38;
    inbox_mount.statx_device_major = 0;
    inbox_mount.statx_device_minor = statx_minor;
    let (job_facts_dir, job_facts_mount) = directory_and_mount(
        "/opt/velnor/job-facts",
        "job-facts",
        303,
        33,
        true,
        true,
        ("btrfs", 0x9123_683e),
    );
    VerifiedProofChannelBinding::from_measured(ProofChannelBindingInput {
        reservation_id: 1,
        operation_id: 2,
        connection_id: "conn-1".to_owned(),
        scale_set_id: 3,
        witness_request_id: 4,
        engine_id: "engine-1".to_owned(),
        docker_root_digest: "b".repeat(64),
        runner_id: 5,
        runner_name: "velnor-runner-1".to_owned(),
        runner_container_id: "c".repeat(64),
        dind_container_id: "d".repeat(64),
        instance_binding_digest: "e".repeat(64),
        metadata_volume_name: "velnor-meta-1".to_owned(),
        metadata_volume_id: "velnor-meta-id-1".to_owned(),
        control: control_mount,
        inbox: inbox_mount,
        job_facts: job_facts_mount,
        observation: ExecutionChannelObservation {
            schema_version: 3,
            operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
            guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
            mount_namespace_id: 4_026_531_840,
            control: control_dir,
            inbox: inbox_dir,
            job_facts: job_facts_dir,
        },
        dind_attachment_absent: true,
    })
}

#[test]
fn public_btrfs_subpath_inbox_and_cleanup_docker_root_device_contracts() -> TestResult<()> {
    let btrfs_inbox = build_inbox_binding(("btrfs", 0x9123_683e), "/dev/vdb1", 41)?;
    assert_eq!(btrfs_inbox.channel_digest().len(), 64);
    assert!(matches!(
        build_inbox_binding(("ext4", 0xef53), "/dev/vdb1", 41),
        Err(HostError::Identity)
    ));
    assert!(matches!(
        build_inbox_binding(("btrfs", 0x9123_683e), "", 41),
        Err(HostError::Identity)
    ));

    let btrfs_cleanup = sample_cleanup_binding(0x9123_683e, [0, 38], [0, 41]);
    assert_eq!(validate_binding(&btrfs_cleanup), Ok(()));
    assert!(docker_root_matches(
        &btrfs_cleanup.docker_root,
        &btrfs_cleanup.docker_root
    ));

    let ext4_split_cleanup = sample_cleanup_binding(0xef53, [0, 38], [0, 41]);
    assert_eq!(
        validate_binding(&ext4_split_cleanup),
        Err("cleanup_docker_root_identity")
    );
    let mut mismatched_statx = btrfs_cleanup.docker_root.clone();
    mismatched_statx.statx_device = [0, 42];
    assert!(!docker_root_matches(
        &btrfs_cleanup.docker_root,
        &mismatched_statx
    ));
    Ok(())
}

fn sample_budget() -> TestResult<velnor_runner_host::ResourceBudget> {
    let config = HostConfig::parse(
        r#"
schema = 2
connections = []

[host]
max_jobs = 1

[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 2147483648
dind_cpu_millicores = 2000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 67108864

[docker]
context = "default"
endpoint = "unix:///var/run/docker.sock"
platform = "linux/amd64"
"#,
    )?;
    Ok(config.resource_budget()?)
}

fn sample_role_charges() -> [(&'static str, u64, u64); 11] {
    [
        ("runner-root", 134_217_728, 4_096),
        ("runner-home", 2_147_483_648, 262_144),
        ("runner-work", 4_294_967_296, 524_288),
        ("runner-tmp", 536_870_912, 32_768),
        ("runner-shm", 67_108_864, 4_096),
        ("shared-run", 67_108_864, 4_096),
        ("dind-root", 134_217_728, 4_096),
        ("dind-data", 4_294_967_296, 262_144),
        ("dind-tmp", 536_870_912, 32_768),
        ("dind-shm", 67_108_864, 4_096),
        ("engine-metadata", 67_108_864, 4_096),
    ]
}

#[test]
fn public_bootstrap_pair_memory_tmpfs_and_cleanup_observer_contracts() -> TestResult<()> {
    let budget = sample_budget()?;
    let charges = sample_role_charges();
    let nonce = "0123456789abcdef0123456789abcdef";
    let dind_id = "d".repeat(64);
    let runner_id = "c".repeat(64);

    let runner_spec = build_bootstrap_pair_create_spec(
        BootstrapContainerRole::Runner,
        nonce,
        "conn-1",
        "sha256:runner-image",
        &format!("container:{dind_id}"),
        budget,
        &charges,
    )?;
    let dind_spec = build_bootstrap_pair_create_spec(
        BootstrapContainerRole::Dind,
        nonce,
        "conn-1",
        "sha256:dind-image",
        "velnor-v0123456789abcdef0123456789abcdef-private",
        budget,
        &charges,
    )?;

    let runner_host = runner_spec
        .config
        .host_config
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing runner host config"))?;
    let dind_host = dind_spec
        .config
        .host_config
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing dind host config"))?;
    assert_eq!(runner_host.memory, Some(9_261_023_232));
    assert_eq!(runner_host.memory_swap, Some(9_261_023_232));
    assert_eq!(dind_host.memory, Some(7_046_430_720));
    assert_eq!(dind_host.memory_swap, Some(7_046_430_720));

    let image_cfg = ImageConfig::default();
    let mut inspected_dind = ContainerInspectResponse {
        id: Some(dind_id.clone()),
        name: Some(format!(
            "/{}",
            dind_spec.options.name.as_deref().unwrap_or_default()
        )),
        image: dind_spec.config.image.clone(),
        config: Some(ContainerConfig {
            image: dind_spec.config.image.clone(),
            labels: dind_spec.config.labels.clone(),
            env: dind_spec.config.env.clone(),
            open_stdin: dind_spec.config.open_stdin,
            stdin_once: dind_spec.config.stdin_once,
            ..Default::default()
        }),
        host_config: dind_spec.config.host_config.clone(),
        ..Default::default()
    };
    if let Some(host) = inspected_dind.host_config.as_mut() {
        host.security_opt = Some(vec!["label=disable".to_owned()]);
        if let Some(tmpfs) = host.tmpfs.as_mut() {
            for options in tmpfs.values_mut() {
                if let Some(stripped) = options.strip_prefix("rw,") {
                    *options = stripped.to_owned();
                }
            }
        }
    }
    verify_pre_jit_created(
        &inspected_dind,
        &dind_id,
        &format!("/{}", dind_spec.options.name.as_deref().unwrap_or_default()),
        &dind_spec,
        &image_cfg,
    )?;

    let mut inspected_runner = ContainerInspectResponse {
        id: Some(runner_id.clone()),
        name: Some(format!(
            "/{}",
            runner_spec.options.name.as_deref().unwrap_or_default()
        )),
        image: runner_spec.config.image.clone(),
        config: Some(ContainerConfig {
            image: runner_spec.config.image.clone(),
            labels: runner_spec.config.labels.clone(),
            env: runner_spec.config.env.clone(),
            open_stdin: runner_spec.config.open_stdin,
            stdin_once: runner_spec.config.stdin_once,
            ..Default::default()
        }),
        host_config: runner_spec.config.host_config.clone(),
        ..Default::default()
    };
    if let Some(host) = inspected_runner.host_config.as_mut() {
        host.security_opt = Some(vec!["label=disable".to_owned()]);
    }
    assert!(matches!(
        verify_pre_jit_created(
            &inspected_runner,
            &runner_id,
            &format!(
                "/{}",
                runner_spec.options.name.as_deref().unwrap_or_default()
            ),
            &runner_spec,
            &image_cfg,
        ),
        Err(HostError::Ownership)
    ));

    let binding = sample_cleanup_binding(0x9123_683e, [0, 38], [0, 41]);
    let request = CleanupObserverRequest {
        schema_version: 1,
        operation_id: nonce.to_owned(),
        binding_sha256: "f".repeat(64),
        binding: binding.clone(),
    };
    let request_json = serde_json::to_string(&request)?;
    let framed = cleanup_observer_request_line(&request_json, nonce)?;
    let lines = framed.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], request_json.as_bytes());
    assert_eq!(
        std::str::from_utf8(lines[1])?,
        format!(r#"{{"operation_id":"{nonce}"}}"#)
    );
    assert!(lines[2].is_empty());

    let mut guest_root = binding.docker_root.clone();
    guest_root.mount_id = 999;
    guest_root.statx_mount_id = 999;
    let record = CleanupObserverRecord {
        schema_version: 1,
        operation_id: nonce.to_owned(),
        binding_sha256: request.binding_sha256.clone(),
        guest_boot_id: Some(binding.expected_guest_boot_id.clone()),
        docker_root: Some(guest_root),
        complete: true,
        owned_references_absent: true,
        task_count: 10,
        namespace_count: 2,
        descriptor_count: 20,
        owned_reference_count: 0,
        absent_volume_mountpoints: binding
            .volume_mountpoints
            .iter()
            .map(|volume| volume.mountpoint.clone())
            .collect(),
        absent_tmpfs_devices: binding.owned_tmpfs_devices.to_vec(),
        absent_mount_namespaces: vec![
            NamespaceIdentity {
                device: binding.runner.namespace_device,
                inode: binding.runner.namespace_inode,
            },
            NamespaceIdentity {
                device: binding.dind.namespace_device,
                inode: binding.dind.namespace_inode,
            },
        ],
        absent_cgroup_paths: Some(binding.cgroup_paths.clone()),
        issues: Vec::new(),
    };
    let mut record_bytes = serde_json::to_vec(&record)?;
    record_bytes.push(b'\n');
    let parsed = parse_cleanup_observer_record(&record_bytes, &request)?;
    validate_cleanup_observer_record(&parsed, &binding, nonce, &request.binding_sha256)?;

    let mut mismatched_fs = record;
    if let Some(root) = mismatched_fs.docker_root.as_mut() {
        root.filesystem_id = 88;
    }
    let mut bad_bytes = serde_json::to_vec(&mismatched_fs)?;
    bad_bytes.push(b'\n');
    assert!(matches!(
        parse_cleanup_observer_record(&bad_bytes, &request),
        Err(HostError::Identity)
    ));
    assert!(matches!(
        validate_cleanup_observer_record(&mismatched_fs, &binding, nonce, &request.binding_sha256),
        Err(HostError::Ownership)
    ));
    Ok(())
}
