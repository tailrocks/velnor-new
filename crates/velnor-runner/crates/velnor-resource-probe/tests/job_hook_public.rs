// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "job_hook_public/job_hook_decisions.rs"]
mod job_hook_decisions;

#[path = "job_hook_public/mountinfo_busy_host.rs"]
mod mountinfo_busy_host;

#[path = "job_hook_public/cleanup_record.rs"]
mod cleanup_record;

#[path = "job_hook_public/cleanup_observer_tmpfs.rs"]
mod cleanup_observer_tmpfs;

#[path = "job_hook_public/cleanup_observer_projection.rs"]
mod cleanup_observer_projection;

#[cfg(target_os = "linux")]
#[path = "job_hook_public/cleanup_observer_scan.rs"]
mod cleanup_observer_scan;

#[cfg(target_os = "linux")]
#[path = "job_hook_public/cleanup_observation_recheck.rs"]
mod cleanup_observation_recheck;

#[cfg(target_os = "linux")]
#[path = "job_hook_public/owned_cgroup_daemon_binding.rs"]
mod owned_cgroup_daemon_binding;

#[cfg(target_os = "linux")]
#[path = "job_hook_public/job_facts_guest_image.rs"]
mod job_facts_guest_image;

use std::error::Error;
use std::fs::{self, File};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use velnor_resource_probe::execution_request::{
    REQUEST_FILE, RequestMetadata, ensure_stable_content, ensure_stable_identity, read_request_file,
};
use velnor_resource_probe::{CpuAncestorLimit, CpuHierarchy, ProbeError, observe_cpu_hierarchy_in};

type TestResult<T> = Result<T, Box<dyn Error>>;

struct ScopedTempDir(std::path::PathBuf);

impl ScopedTempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "velnor-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for ScopedTempDir {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn assert_mid_read_stability_contracts(meta: RequestMetadata, payload: &[u8]) -> TestResult<()> {
    ensure_stable_identity(meta, meta, meta, meta)?;
    ensure_stable_content(payload, payload)?;
    let mut swapped_inode = meta;
    swapped_inode.inode = meta.inode.wrapping_add(1);
    assert!(matches!(
        ensure_stable_identity(meta, meta, swapped_inode, swapped_inode),
        Err(ProbeError::Invalid("execution_request_changed"))
    ));
    assert!(matches!(
        ensure_stable_content(payload, b"{\"schema_version\":2}\n"),
        Err(ProbeError::Invalid("execution_request_changed"))
    ));
    Ok(())
}

#[test]
fn public_inbox_request_poll_returns_none_when_published_after_initial_enoent_and_reads_on_next_poll()
-> TestResult<()> {
    let temp = ScopedTempDir::new("inbox-request-poll")?;
    let inbox_path = temp.path().join("inbox");
    fs::create_dir(&inbox_path)?;
    fs::set_permissions(&inbox_path, fs::Permissions::from_mode(0o700))?;
    let inbox = File::open(&inbox_path)?;
    let meta = inbox.metadata()?;
    let (uid, gid) = (meta.uid(), meta.gid());

    let request_path = inbox_path.join(REQUEST_FILE);
    let payload = b"{\"schema_version\":1}\n";
    let mut observed_meta = None;
    for round in 0..64_usize {
        if request_path.exists() {
            fs::remove_file(&request_path)?;
        }
        let staged_path = inbox_path.join(format!(".request.{round}.tmp"));
        fs::write(&staged_path, payload)?;
        fs::set_permissions(&staged_path, fs::Permissions::from_mode(0o600))?;

        let barrier = Arc::new(Barrier::new(2));
        let empty_polls = Arc::new(AtomicUsize::new(0));
        let poller_inbox = inbox.try_clone()?;
        let poller_barrier = Arc::clone(&barrier);
        let poller_polls = Arc::clone(&empty_polls);

        let poller = thread::spawn(move || -> Result<(Vec<u8>, RequestMetadata), ProbeError> {
            poller_barrier.wait();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some((bytes, read_meta)) = read_request_file(&poller_inbox, uid, gid)? {
                    return Ok((bytes, read_meta));
                }
                poller_polls.fetch_add(1, Ordering::Release);
                if Instant::now() >= deadline {
                    return Err(ProbeError::Invalid("test_poll_timeout"));
                }
            }
        });

        barrier.wait();
        let wait_deadline = Instant::now() + Duration::from_secs(5);
        while empty_polls.load(Ordering::Acquire) <= round % 4 {
            if Instant::now() >= wait_deadline {
                return Err("timed out waiting for initial empty poll".into());
            }
            thread::yield_now();
        }
        fs::rename(&staged_path, &request_path)?;

        let (read_bytes, read_meta) = poller
            .join()
            .map_err(|_| "poller thread panicked")?
            .map_err(|err| format!("round {round} failed: {err:?}"))?;
        assert_eq!(read_bytes, payload);
        assert_eq!(read_meta.size, payload.len() as u64);
        observed_meta = Some(read_meta);
    }

    assert_mid_read_stability_contracts(observed_meta.ok_or("missing observed meta")?, payload)?;

    fs::write(&request_path, payload)?;
    fs::set_permissions(&request_path, fs::Permissions::from_mode(0o644))?;
    assert!(matches!(
        read_request_file(&inbox, uid, gid),
        Err(ProbeError::Invalid("execution_request_file"))
    ));
    Ok(())
}

#[test]
fn public_observe_cpu_hierarchy_admits_root_without_cpu_max_and_rejects_nonroot_or_leaf_missing_cpu_max()
-> TestResult<()> {
    let dir = ScopedTempDir::new("cpu_hierarchy_root_cpu_max")?;
    let root = dir.path();
    let slice = root.join("system.slice");
    let leaf = slice.join("docker-probe.scope");
    fs::create_dir_all(&leaf)?;

    fs::write(root.join("cpuset.cpus.effective"), "0-3\n")?;
    fs::write(slice.join("cpuset.cpus.effective"), "0-3\n")?;
    fs::write(slice.join("cpu.max"), "max 100000\n")?;
    fs::write(leaf.join("cpuset.cpus.effective"), "0-1\n")?;
    fs::write(leaf.join("cpu.max"), "25000 100000\n")?;

    let observed = observe_cpu_hierarchy_in(root, "/system.slice/docker-probe.scope")?;
    assert_eq!(
        observed,
        CpuHierarchy {
            leaf_cpuset_cpus_count: 2,
            ancestors: vec![
                CpuAncestorLimit {
                    path: "/".to_owned(),
                    cpuset_cpus_count: 4,
                    cpu_quota_micros: None,
                    cpu_period_micros: 100_000,
                },
                CpuAncestorLimit {
                    path: "/system.slice".to_owned(),
                    cpuset_cpus_count: 4,
                    cpu_quota_micros: None,
                    cpu_period_micros: 100_000,
                },
            ],
            leaf_quota_micros: Some(25_000),
            leaf_period_micros: 100_000,
        }
    );

    fs::write(root.join("cpu.max"), "50000 100000\n")?;
    let with_root_limit = observe_cpu_hierarchy_in(root, "/system.slice/docker-probe.scope")?;
    assert_eq!(
        with_root_limit.ancestors[0],
        CpuAncestorLimit {
            path: "/".to_owned(),
            cpuset_cpus_count: 4,
            cpu_quota_micros: Some(50_000),
            cpu_period_micros: 100_000,
        }
    );

    fs::write(root.join("cpu.max"), "invalid\n")?;
    assert!(matches!(
        observe_cpu_hierarchy_in(root, "/system.slice/docker-probe.scope"),
        Err(ProbeError::Invalid("cpu_max_format"))
    ));

    fs::remove_file(root.join("cpu.max"))?;
    assert!(matches!(
        observe_cpu_hierarchy_in(root, "/"),
        Err(ProbeError::Read("cpu.max", _))
    ));

    fs::remove_file(slice.join("cpu.max"))?;
    assert!(matches!(
        observe_cpu_hierarchy_in(root, "/system.slice/docker-probe.scope"),
        Err(ProbeError::Read("cpu.max", _))
    ));

    fs::write(slice.join("cpu.max"), "max 100000\n")?;
    fs::remove_file(leaf.join("cpu.max"))?;
    assert!(matches!(
        observe_cpu_hierarchy_in(root, "/system.slice/docker-probe.scope"),
        Err(ProbeError::Read("cpu.max", _))
    ));
    Ok(())
}
