// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fmt::Write as _,
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
        Self::launch(name, 1)
    }

    fn launch(name: &str, mounts: usize) -> TestResult<Self> {
        let mounts = vec!["mount -t tmpfs tmpfs \"$1\""; mounts].join(" && ");
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
                &format!("{mounts} && echo ready && exec sleep 300"),
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

    fn volume(&self) -> CleanupVolume {
        CleanupVolume {
            role: "runner-home".to_owned(),
            mountpoint: self.directory.display().to_string(),
            mount_device: self.mount_device,
            statx_device: self.mount_device,
            inode: self.mount_inode,
        }
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

fn live_process(pid: u32, namespace: ([u32; 2], u64)) -> TestResult<ProcessIdentity> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let start_ticks = stat
        .rsplit_once(')')
        .and_then(|(_, fields)| fields.split_ascii_whitespace().nth(19))
        .ok_or("process start ticks are missing")?
        .parse()?;
    Ok(ProcessIdentity {
        start_ticks,
        ..process(pid, namespace)
    })
}

fn scan_runner_identity(runner: ProcessIdentity) -> TestResult<CleanupObserverRecord> {
    let mut request = request(Scenario {
        runner_namespace: (runner.namespace_device, runner.namespace_inode),
        volume_override: None,
    })?;
    request.binding.runner = runner;
    let runtime = fs::read_to_string("/proc/self/mountinfo")?
        .lines()
        .map(|line| line.split_ascii_whitespace().collect::<Vec<_>>())
        .find(|fields| fields.get(4).copied() == Some("/dev"))
        .and_then(|fields| fields.get(2).copied())
        .and_then(|device| device.split_once(':'))
        .map(|(major, minor)| Ok::<_, Box<dyn Error>>([major.parse()?, minor.parse()?]))
        .transpose()?
        .ok_or("observer runtime mount is missing")?;
    request.binding.owned_tmpfs_devices[0] = runtime;
    request.binding_sha256 = Sha256::digest(serde_json::to_vec(&request.binding)?)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _written = write!(hex, "{byte:02x}");
            hex
        });
    Ok(observe_cleanup(request)?)
}

fn assert_observer_namespace_unowned(record: &CleanupObserverRecord) {
    assert_eq!(record.owned_reference_count, 0, "{record:?}");
    assert!(
        !record
            .issues
            .iter()
            .any(|issue| issue == "observer_in_pair_namespace"),
        "{record:?}"
    );
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
    let digest = Sha256::digest(serde_json::to_vec(&binding)?).iter().fold(
        String::new(),
        |mut hex, byte| {
            let _written = write!(hex, "{byte:02x}");
            hex
        },
    );
    Ok(CleanupObserverRequest {
        schema_version: 1,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        binding_sha256: digest,
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
fn a_descriptor_held_outside_the_pair_namespace_is_an_owned_reference() -> TestResult<()> {
    let mut pair = PairNamespace::start("held-namespace")?;
    let mut holder = Command::new("sh")
        .args(["-c", "exec 3<\"$1\" && echo held && exec sleep 300", "sh"])
        .arg(format!("/proc/{}/ns/mnt", pair.pid()))
        .stdout(Stdio::piped())
        .spawn()?;
    let held = BufReader::new(holder.stdout.take().ok_or("no stdout")?)
        .lines()
        .next();
    pair.child.kill().and_then(|()| pair.child.wait())?;
    let record = scan(Scenario {
        runner_namespace: pair.namespace,
        volume_override: None,
    });
    holder.kill().and_then(|()| holder.wait())?;
    assert_eq!(held.transpose()?.as_deref(), Some("held"));
    assert!(record?.owned_reference_count >= 1);
    Ok(())
}

#[test]
fn a_pair_volume_mounted_in_another_namespace_is_an_owned_reference() -> TestResult<()> {
    let pair = PairNamespace::start("volume-mount")?;
    let record = scan(Scenario {
        runner_namespace: untouched_namespace(),
        volume_override: Some(pair.volume()),
    })?;
    assert_eq!(record.owned_reference_count, 1, "{record:?}");
    Ok(())
}

#[test]
fn a_pair_volume_covered_by_another_mount_is_an_owned_reference() -> TestResult<()> {
    let pair = PairNamespace::launch("covered-volume", 2)?;
    let record = scan(Scenario {
        runner_namespace: untouched_namespace(),
        volume_override: Some(pair.volume()),
    })?;
    assert_eq!(record.owned_reference_count, 1, "{record:?}");
    Ok(())
}

#[test]
fn an_observer_inside_a_pair_namespace_is_refused_and_counts_its_own_tasks() -> TestResult<()> {
    let own = identity("/proc/self/ns/mnt")?;
    let record = scan_runner_identity(live_process(std::process::id(), own)?)?;
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

#[test]
fn a_destroyed_pair_namespace_number_reused_by_the_observer_is_not_owned() -> TestResult<()> {
    let own = identity("/proc/self/ns/mnt")?;
    let mut child = Command::new("sleep").arg("300").spawn()?;
    let runner = live_process(child.id(), own)?;
    child.kill()?;
    child.wait()?;
    assert_observer_namespace_unowned(&scan_runner_identity(runner)?);
    Ok(())
}

#[test]
fn a_reused_pair_pid_does_not_make_the_observer_namespace_or_runtime_mounts_owned() -> TestResult<()>
{
    let own = identity("/proc/self/ns/mnt")?;
    let mut runner = live_process(std::process::id(), own)?;
    runner.start_ticks += 1;
    assert_observer_namespace_unowned(&scan_runner_identity(runner)?);
    Ok(())
}

#[test]
fn a_live_pair_process_in_another_namespace_does_not_protect_the_observer_namespace()
-> TestResult<()> {
    let pair = PairNamespace::start("namespace-mismatch")?;
    let own = identity("/proc/self/ns/mnt")?;
    assert_ne!(pair.namespace, own);
    let runner = live_process(pair.pid(), own)?;
    assert_observer_namespace_unowned(&scan_runner_identity(runner)?);
    Ok(())
}

#[path = "cleanup_observer_unreadable_identity.rs"]
mod cleanup_observer_unreadable_identity;
