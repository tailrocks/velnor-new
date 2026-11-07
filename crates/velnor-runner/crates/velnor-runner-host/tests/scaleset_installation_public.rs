// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "scaleset_installation_public/attestation_and_logs.rs"]
mod attestation_and_logs;
#[path = "scaleset_installation_public/runtime_and_policy.rs"]
mod runtime_and_policy;

use std::{error::Error, fs, process::Command};

use serde_json::{Value, json};
use velnor_resource_probe::execution_channel_wire::{
    ExecutionChannelDirectory, ExecutionChannelObservation,
};
use velnor_resource_probe::{CpuAncestorLimit, InodePolicy, ProbeRecord};
use velnor_runner_host::resource_probe_release::validate_smoke;
use velnor_runner_host::{
    HostError, ProofChannelBindingInput, ProofChannelMount, VerifiedProofChannelBinding,
    WorkerImage, verify_worker_build_source,
};

type TestResult<T> = Result<T, Box<dyn Error>>;

const SOURCE_SHA: &str = "1111111111111111111111111111111111111111";
const WRONG_SOURCE_SHA: &str = "2222222222222222222222222222222222222222";
const MANIFEST_DIGEST: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

fn schema5_record_bytes(schema_version: u8) -> TestResult<Vec<u8>> {
    let record = ProbeRecord {
        schema_version,
        docker_root_mount_target: "/velnor/docker-root".to_owned(),
        docker_root_mount_source: "/dev/vda1".to_owned(),
        docker_root_mount_root: "/var/lib/docker".to_owned(),
        docker_root_mount_id: 42,
        docker_root_device_major: 254,
        docker_root_device_minor: 1,
        docker_root_statx_device_major: 254,
        docker_root_statx_device_minor: 1,
        docker_root_fs_type_magic: 61267,
        docker_root_fs_type_name: "ext4".to_owned(),
        docker_root_filesystem_id: 9001,
        guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        docker_root_free_bytes: 50_000_000_000,
        docker_root_total_bytes: 100_000_000_000,
        docker_root_available_bytes: 45_000_000_000,
        docker_root_inode_total: 1_000_000,
        docker_root_inode_free: 900_000,
        docker_root_inode_available: 900_000,
        docker_root_inode_policy: InodePolicy::Finite,
        memory_available_bytes: 8_589_934_592,
        load_milli: 250,
        memory_psi_some_avg10_bps: Some(0),
        probe_cgroup_path: "/docker/probe-1".to_owned(),
        probe_cgroup_memory_max_bytes: Some(67_108_864),
        probe_cgroup_memory_current_bytes: 4_194_304,
        probe_cgroup_swap_max_bytes: Some(0),
        probe_cgroup_swap_current_bytes: 0,
        probe_cgroup_cpu_quota_micros: Some(25_000),
        probe_cgroup_cpu_period_micros: 100_000,
        probe_cgroup_cpuset_cpus_count: 4,
        cpu_ancestor_limits: vec![
            CpuAncestorLimit {
                path: "/".to_owned(),
                cpuset_cpus_count: 4,
                cpu_quota_micros: None,
                cpu_period_micros: 100_000,
            },
            CpuAncestorLimit {
                path: "/docker".to_owned(),
                cpuset_cpus_count: 4,
                cpu_quota_micros: None,
                cpu_period_micros: 100_000,
            },
        ],
        probe_host_pid: 1234,
        probe_namespace_pid: 1234,
        probe_cgroup_pids_max: Some(64),
    };
    let mut bytes = serde_json::to_vec(&record)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[test]
fn public_validate_smoke_admits_schema5_and_rejects_schema1_or_removed_subcommand() -> TestResult<()>
{
    let temp = tempfile::tempdir()?;
    let root_dir = temp.path().join("docker-root");
    fs::create_dir(&root_dir)?;
    let valid_path = temp.path().join("probe-v5.json");
    fs::write(&valid_path, schema5_record_bytes(5)?)?;

    validate_smoke(&valid_path, &root_dir)?;

    let cli_ok = Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .arg("validate-smoke")
        .arg(&valid_path)
        .arg(&root_dir)
        .output()?;
    assert!(
        cli_ok.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_ok.stderr)
    );

    let schema1_path = temp.path().join("probe-v1.json");
    fs::write(&schema1_path, schema5_record_bytes(1)?)?;
    assert!(matches!(
        validate_smoke(&schema1_path, &root_dir),
        Err(HostError::Identity)
    ));

    let missing_root = temp.path().join("missing-root");
    assert!(matches!(
        validate_smoke(&valid_path, &missing_root),
        Err(HostError::Path)
    ));

    let removed_subcommand = Command::new(env!("CARGO_BIN_EXE_velnor-resource-probe-release"))
        .arg("validate-smoke-schema5")
        .arg(&valid_path)
        .arg(&root_dir)
        .output()?;
    assert!(!removed_subcommand.status.success());
    Ok(())
}

fn slsa_statement(image: WorkerImage, locals: &[&str], source_sha: &str) -> Value {
    let (image_name, source_context) = match image {
        WorkerImage::Runner => ("velnor-runner", "images/runner/ubuntu-26.04"),
        WorkerImage::Dind => ("velnor-dind", "images/dind"),
    };
    let locals_json = locals
        .iter()
        .map(|name| json!({ "name": *name }))
        .collect::<Vec<_>>();
    let mut vcs = serde_json::Map::new();
    vcs.insert("revision".to_owned(), json!(source_sha));
    vcs.insert(
        "source".to_owned(),
        json!("https://github.com/tailrocks/velnor-new"),
    );
    for name in locals {
        let dir_value = if *name == "context" || *name == "dockerfile" {
            source_context
        } else {
            *name
        };
        vcs.insert(format!("localdir:{name}"), json!(dir_value));
    }
    let mut request = serde_json::Map::from_iter([
        ("frontend".to_owned(), json!("dockerfile.v0")),
        ("locals".to_owned(), json!(locals_json)),
    ]);
    if locals.len() > 2 {
        let mut args = serde_json::Map::from_iter([
            ("build-arg:VELNOR_SOURCE_SHA".to_owned(), json!(source_sha)),
            (
                "frontend.caps".to_owned(),
                json!("moby.buildkit.frontend.contexts+forward"),
            ),
        ]);
        for name in locals
            .iter()
            .filter(|name| !matches!(**name, "context" | "dockerfile"))
        {
            args.insert(format!("context:{name}"), json!(format!("local:{name}")));
        }
        request.insert("args".to_owned(), Value::Object(args));
        request.insert("compatibilityVersion".to_owned(), json!(30));
    }
    json!({
        "_type": "https://in-toto.io/Statement/v0.1",
        "predicateType": "https://slsa.dev/provenance/v1",
        "subject": [{
            "name": format!("pkg:docker/{image_name}@linux-amd64?platform=linux%2Famd64"),
            "digest": {
                "sha256": MANIFEST_DIGEST.trim_start_matches("sha256:")
            }
        }],
        "predicate": {
            "buildDefinition": {
                "buildType": "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md",
                "externalParameters": {
                    "configSource": {
                        "path": "Dockerfile"
                    },
                    "request": Value::Object(request)
                },
                "internalParameters": {
                    "builderPlatform": "linux/amd64"
                },
                "resolvedDependencies": []
            },
            "runDetails": {
                "metadata": {
                    "buildkit_metadata": {
                        "vcs": Value::Object(vcs)
                    }
                }
            }
        }
    })
}

const RUNNER_NAMED_LOCALS: [&str; 6] = [
    "context",
    "dockerfile",
    "resource_probe",
    "storage_init",
    "job_start_hook",
    "job_started_hook",
];

#[test]
fn public_verify_worker_build_source_accepts_base_and_named_contexts_and_rejects_mismatches()
-> TestResult<()> {
    for (image, locals) in [
        (WorkerImage::Runner, ["context", "dockerfile"].as_slice()),
        (WorkerImage::Runner, &RUNNER_NAMED_LOCALS),
        (
            WorkerImage::Dind,
            ["context", "dockerfile", "resource_probe"].as_slice(),
        ),
    ] {
        let statement = slsa_statement(image, locals, SOURCE_SHA);
        verify_worker_build_source(&statement, image, SOURCE_SHA, MANIFEST_DIGEST, 1024)?;
    }
    let runner_named = slsa_statement(WorkerImage::Runner, &RUNNER_NAMED_LOCALS, SOURCE_SHA);

    let dind_with_runner_contexts =
        slsa_statement(WorkerImage::Dind, &RUNNER_NAMED_LOCALS, SOURCE_SHA);
    let mut missing_localdir = runner_named.clone();
    missing_localdir
        .pointer_mut("/predicate/runDetails/metadata/buildkit_metadata/vcs")
        .and_then(Value::as_object_mut)
        .ok_or("vcs map")?
        .remove("localdir:job_start_hook");
    let mut unexpected_request_key = runner_named.clone();
    unexpected_request_key["predicate"]["buildDefinition"]["externalParameters"]["request"]["unexpected"] =
        json!(true);
    let mut wrong_compat_version = runner_named.clone();
    wrong_compat_version["predicate"]["buildDefinition"]["externalParameters"]["request"]["compatibilityVersion"] =
        json!(20);
    for (image, statement, sha) in [
        (WorkerImage::Dind, &dind_with_runner_contexts, SOURCE_SHA),
        (WorkerImage::Runner, &missing_localdir, SOURCE_SHA),
        (WorkerImage::Runner, &unexpected_request_key, SOURCE_SHA),
        (WorkerImage::Runner, &wrong_compat_version, SOURCE_SHA),
        (WorkerImage::Runner, &runner_named, WRONG_SOURCE_SHA),
    ] {
        assert!(matches!(
            verify_worker_build_source(statement, image, sha, MANIFEST_DIGEST, 1024),
            Err(HostError::Docker)
        ));
    }
    Ok(())
}

pub(crate) fn directory_and_mount(
    target: &str,
    subpath: &str,
    inode: u64,
    mount_id: u64,
    read_only: bool,
    executable_mount: bool,
    fs_type: (&str, u64),
) -> (ExecutionChannelDirectory, ProofChannelMount) {
    let (owner, mode, mount_source) = if read_only {
        (0, 0o555, "/dev/vda1")
    } else {
        (1000, 0o700, "tmpfs")
    };
    let dir = ExecutionChannelDirectory {
        target: target.to_owned(),
        inode,
        owner_uid: owner,
        owner_gid: owner,
        mode,
        mount_id,
        statx_mount_id: mount_id,
        mount_root: format!("/{subpath}"),
        mount_source: mount_source.to_owned(),
        mount_device_major: 254,
        mount_device_minor: 1,
        statx_device_major: 254,
        statx_device_minor: 1,
        filesystem_type_magic: fs_type.1,
        filesystem_type_name: fs_type.0.to_owned(),
        filesystem_id: 77,
        read_only,
        executable_mount,
    };
    let mount = ProofChannelMount {
        target: target.to_owned(),
        subpath: subpath.to_owned(),
        mount_id,
        device_major: 254,
        device_minor: 1,
        statx_device_major: 254,
        statx_device_minor: 1,
        fs_type_magic: fs_type.1,
        filesystem_id: 77,
        filesystem_instance_digest: "a".repeat(64),
        directory_inode: inode,
        owner_uid: owner,
        owner_gid: owner,
        mode,
        read_only,
        no_copy: true,
    };
    (dir, mount)
}

fn build_channel_binding(
    control_exec: bool,
    inbox_exec: bool,
    job_facts_read_only: bool,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let (control_dir, control_mount) = directory_and_mount(
        "/opt/velnor/source-proof/control",
        "source-proof/control",
        101,
        11,
        true,
        control_exec,
        ("ext4", 0xef53),
    );
    let (inbox_dir, inbox_mount) = directory_and_mount(
        "/opt/velnor/source-proof/inbox",
        "source-proof/inbox",
        202,
        22,
        false,
        inbox_exec,
        ("tmpfs", 0x0102_1994),
    );
    let (mut job_facts_dir, mut job_facts_mount) = directory_and_mount(
        "/opt/velnor/job-facts",
        "job-facts",
        303,
        33,
        true,
        true,
        ("ext4", 0xef53),
    );
    job_facts_dir.read_only = job_facts_read_only;
    job_facts_mount.read_only = job_facts_read_only;

    let observation = ExecutionChannelObservation {
        schema_version: 3,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        mount_namespace_id: 4_026_531_840,
        control: control_dir,
        inbox: inbox_dir,
        job_facts: job_facts_dir,
    };
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
        observation,
        dind_attachment_absent: true,
    })
}

#[test]
fn public_proof_channel_binding_accepts_executable_volume_mounts_and_rejects_writable_job_facts()
-> TestResult<()> {
    let non_exec = build_channel_binding(false, false, true)?;
    assert_eq!(non_exec.channel_digest().len(), 64);

    let exec_mounts = build_channel_binding(true, true, true)?;
    assert_eq!(exec_mounts.channel_digest().len(), 64);

    assert!(matches!(
        build_channel_binding(true, true, false),
        Err(HostError::Identity)
    ));
    Ok(())
}
