// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::MetadataExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
};

use sha2::{Digest, Sha256};
use velnor_resource_probe::cleanup_observer::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupObserverRecord,
    CleanupObserverRequest, CleanupVolume, ProcessIdentity, observe_cleanup,
};

type TestResult<T> = Result<T, Box<dyn Error>>;

const FAKE_NAMESPACE_INODE: u64 = 9_000_001;

struct PairNamespace {
    child: Child,
    directory: PathBuf,
    namespace: ([u32; 2], u64),
    mount_device: [u32; 2],
    mount_inode: u64,
}

impl PairNamespace {
    fn start(name: &str) -> TestResult<Self> {
        let directory = std::env::temp_dir().join(format!("velnor-scan-{name}"));
        fs::create_dir_all(&directory)?;
        let mut command = Command::new("unshare");
        if rustix::process::geteuid().is_root() {
            command.arg("--mount");
        } else {
            command.args(["--user", "--map-root-user", "--mount"]);
        }
        let mut child = command
            .args([
                "sh",
                "-c",
                "mount -t tmpfs tmpfs \"$1\" && echo ready && exec sleep 300",
                "sh",
            ])
            .arg(&directory)
            .stdout(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().ok_or("no child stdout")?;
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line)?;
        if line.trim() != "ready" {
            return Err("the child could not enter a new mount namespace".into());
        }
        let pid = child.id();
        let namespace = identity(&format!("/proc/{pid}/ns/mnt"))?;
        let inside = fs::metadata(format!("/proc/{pid}/root{}", directory.display()))?;
        let mountinfo = fs::read_to_string(format!("/proc/{pid}/mountinfo"))?;
        let mount_device = mountinfo
            .lines()
            .map(|line| line.split_ascii_whitespace().collect::<Vec<_>>())
            .find(|fields| fields.get(4).copied() == directory.to_str())
            .and_then(|fields| fields.get(2).copied().map(str::to_owned))
            .and_then(|value| {
                let (major, minor) = value.split_once(':')?;
                Some([major.parse().ok()?, minor.parse().ok()?])
            })
            .ok_or("the child tmpfs mount is missing")?;
        Ok(Self {
            child,
            directory,
            namespace,
            mount_device,
            mount_inode: inside.ino(),
        })
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for PairNamespace {
    fn drop(&mut self) {
        let _killed = self.child.kill();
        let _waited = self.child.wait();
        let _removed = fs::remove_dir(&self.directory);
    }
}

fn identity(path: &str) -> TestResult<([u32; 2], u64)> {
    let metadata = fs::metadata(path)?;
    let device = metadata.dev();
    Ok((
        [rustix::fs::major(device), rustix::fs::minor(device)],
        metadata.ino(),
    ))
}

fn process(pid: u32, namespace: ([u32; 2], u64)) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        start_ticks: 1,
        namespace_device: namespace.0,
        namespace_inode: namespace.1,
    }
}

fn fake_volume(role: &str, minor: u32) -> CleanupVolume {
    CleanupVolume {
        role: role.to_owned(),
        mountpoint: format!("/var/lib/docker/volumes/{role}/_data"),
        mount_device: [4090, minor],
        statx_device: [4090, minor],
        inode: 1,
    }
}

struct Scenario {
    runner_namespace: ([u32; 2], u64),
    volume_override: Option<CleanupVolume>,
}

fn request(scenario: Scenario) -> TestResult<CleanupObserverRequest> {
    let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim_end()
        .to_owned();
    let mut home = fake_volume("runner-home", 2);
    if let Some(volume) = scenario.volume_override {
        home = volume;
    }
    let binding = CleanupObserverBinding {
        expected_guest_boot_id: boot_id,
        metadata_job_facts_inode: None,
        docker_root: CleanupDockerRootIdentity {
            target: "/velnor/docker-root".to_owned(),
            mount_id: 1,
            statx_mount_id: 1,
            mount_root: "/docker".to_owned(),
            mount_device: [4091, 1],
            statx_device: [4091, 1],
            filesystem_type_magic: 0x9123_683e,
            filesystem_id: 7,
            read_only: true,
        },
        daemon: process(1, ([0, 4], FAKE_NAMESPACE_INODE + 1)),
        runner: process(2, scenario.runner_namespace),
        dind: process(3, ([0, 4], FAKE_NAMESPACE_INODE + 2)),
        cgroup_paths: ["/scan-a".to_owned(), "/scan-b".to_owned()],
        volume_mountpoints: vec![
            fake_volume("shared-run", 1),
            home,
            fake_volume("runner-work", 3),
            fake_volume("dind-data", 4),
        ],
        owned_tmpfs_devices: std::array::from_fn(|index| {
            [4000, u32::try_from(index + 1).unwrap_or(1)]
        }),
    };
    let digest = Sha256::digest(serde_json::to_vec(&binding)?);
    Ok(CleanupObserverRequest {
        schema_version: 1,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        binding_sha256: digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        binding,
    })
}

fn scan(scenario: Scenario) -> TestResult<CleanupObserverRecord> {
    Ok(observe_cleanup(request(scenario)?)?)
}

fn untouched_namespace() -> ([u32; 2], u64) {
    ([0, 4], FAKE_NAMESPACE_INODE)
}

#[test]
fn a_host_with_no_pair_namespace_or_volume_has_no_owned_references() -> TestResult<()> {
    let _held = PairNamespace::start("untouched")?;
    let record = scan(Scenario {
        runner_namespace: untouched_namespace(),
        volume_override: None,
    })?;
    assert_eq!(record.owned_reference_count, 0);
    Ok(())
}

#[test]
fn a_task_in_the_runner_mount_namespace_is_an_owned_reference() -> TestResult<()> {
    let pair = PairNamespace::start("runner-namespace")?;
    let record = scan(Scenario {
        runner_namespace: pair.namespace,
        volume_override: None,
    })?;
    assert!(
        record.owned_reference_count >= 1 && !record.owned_references_absent,
        "task {} in the runner namespace was not counted: {record:?}",
        pair.pid()
    );
    Ok(())
}

#[test]
fn a_pair_volume_mounted_in_another_namespace_is_an_owned_reference() -> TestResult<()> {
    let pair = PairNamespace::start("volume-mount")?;
    let record = scan(Scenario {
        runner_namespace: untouched_namespace(),
        volume_override: Some(CleanupVolume {
            role: "runner-home".to_owned(),
            mountpoint: pair.directory.display().to_string(),
            mount_device: pair.mount_device,
            statx_device: pair.mount_device,
            inode: pair.mount_inode,
        }),
    })?;
    assert_eq!(record.owned_reference_count, 1, "{record:?}");
    Ok(())
}

#[test]
fn an_observer_inside_a_pair_namespace_is_refused_and_counts_its_own_tasks() -> TestResult<()> {
    let own = identity("/proc/self/ns/mnt")?;
    let record = scan(Scenario {
        runner_namespace: own,
        volume_override: None,
    })?;
    assert!(
        record.owned_reference_count >= 1
            && !record.owned_references_absent
            && record
                .issues
                .iter()
                .any(|issue| issue == "observer_in_pair_namespace"),
        "{record:?}"
    );
    Ok(())
}
