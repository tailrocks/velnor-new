// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
};

use velnor_resource_probe::{
    ProbeError,
    worker_binding::{OwnedCgroupRequest, observe_owned_cgroups},
};

use super::{ScopedTempDir, TestResult};

const CHILD_ENV: &str = "VELNOR_DAEMON_BINDING_CHILD";
const SETUP: &str = r#"set -eu
mkdir -p /velnor/docker-root /var/lib/docker
mount -t tmpfs -o ro tmpfs /velnor/docker-root
mount --bind /velnor/docker-root /var/lib/docker
umount -R /sys/fs/cgroup 2>/dev/null || true
mount -t cgroup2 -o ro none /sys/fs/cgroup
VELNOR_DAEMON_BINDING_CHILD=1 exec "$@""#;

const ROOTLESS_SETUP: &str = r#"set -eu
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
base=$1
exe=$2
shift 2
newroot=$base/newroot
mkdir -p "$newroot"
mount -t tmpfs tmpfs "$newroot"
mkdir -p "$newroot/proc" "$newroot/dev" "$newroot/tmp" "$newroot/usr" "$newroot/etc" \
    "$newroot/velnor/docker-root" "$newroot/var/lib/docker" "$newroot/put_old"
mount --bind /proc "$newroot/proc"
mount --bind /dev "$newroot/dev"
mount --bind /usr "$newroot/usr"
mount --bind /etc "$newroot/etc"
mount -t tmpfs tmpfs "$newroot/sys"
mkdir -p "$newroot/sys/fs/cgroup"
mount -t cgroup2 -o ro none "$newroot/sys/fs/cgroup"
exedir=$(dirname "$exe")
mkdir -p "$newroot$exedir"
mount --bind "$exedir" "$newroot$exedir"
for entry in bin lib lib64 sbin; do
    if [ -L "/$entry" ]; then
        target=$(readlink "/$entry")
        rmdir "$newroot/$entry" 2>/dev/null || true
        mkdir -p "$newroot/$(dirname "$target")"
        ln -s "$target" "$newroot/$entry"
    else
        mkdir -p "$newroot/$entry"
        mount --bind "/$entry" "$newroot/$entry"
    fi
done
mount -t tmpfs -o ro tmpfs "$newroot/velnor/docker-root"
mount --bind "$newroot/velnor/docker-root" "$newroot/var/lib/docker"
cd "$newroot"
pivot_root . put_old
cd /
TMPDIR=/tmp
export TMPDIR
HOME=/tmp
export HOME
VELNOR_DAEMON_BINDING_CHILD=1
export VELNOR_DAEMON_BINDING_CHILD
exec "$exe" "$@""#;

#[test]
fn owned_cgroup_observation_refuses_a_substituted_dockerd_while_the_real_binary_is_unlinked()
-> TestResult<()> {
    if std::env::var_os(CHILD_ENV).is_some() {
        return observe_with_unlinked_real_dockerd();
    }
    let name = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, rest)| rest);
    let exact = format!(
        "{name}::owned_cgroup_observation_refuses_a_substituted_dockerd_while_the_real_binary_is_unlinked"
    );
    let current_exe = std::env::current_exe()?;
    if rustix::process::geteuid().is_root() {
        let created_velnor = !Path::new("/velnor").exists();
        let status = Command::new("unshare")
            .args([
                "--mount",
                "--propagation",
                "private",
                "sh",
                "-c",
                SETUP,
                "sh",
            ])
            .arg(&current_exe)
            .args(["--exact", &exact, "--nocapture"])
            .status()?;
        drop(fs::remove_dir("/velnor/docker-root"));
        if created_velnor {
            drop(fs::remove_dir("/velnor"));
        }
        if !status.success() {
            return Err("the observation in the private mount namespace failed".into());
        }
        return Ok(());
    }
    let base = ScopedTempDir::new("daemon-binding-rootless")?;
    let probe = Command::new("unshare")
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--cgroup",
            "sh",
            "-c",
            "set -eu; mkdir -p \"$1/p\"; mount --bind /proc \"$1/p\"",
            "sh",
        ])
        .arg(base.path())
        .output()?;
    if !probe.status.success() {
        eprintln!("skipped: this host cannot bind-mount /proc in an unprivileged user namespace");
        return Ok(());
    }
    let status = Command::new("unshare")
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--propagation",
            "private",
            "--cgroup",
            "sh",
            "-c",
            ROOTLESS_SETUP,
            "sh",
        ])
        .arg(base.path())
        .arg(&current_exe)
        .args(["--exact", &exact, "--nocapture"])
        .status()?;
    if !status.success() {
        return Err("the observation in the private mount namespace failed".into());
    }
    Ok(())
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        drop(self.0.kill());
        drop(self.0.wait());
    }
}

fn spawn_dockerd(directory: &Path, label: &str) -> TestResult<(Daemon, std::path::PathBuf)> {
    let folder = directory.join(label);
    fs::create_dir_all(&folder)?;
    let binary = folder.join("dockerd");
    fs::copy("/bin/sleep", &binary)?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
    let child = Command::new(&binary)
        .arg("300")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok((Daemon(child), binary))
}

fn request(real: u32) -> OwnedCgroupRequest {
    OwnedCgroupRequest {
        schema_version: 2,
        operation_id: "a".repeat(32),
        runner_container_id: "b".repeat(64),
        runner_init_pid: real + 1_000_001,
        runner_init_start_ticks: 1,
        runner_helper_pid: real + 1_000_002,
        runner_helper_start_ticks: 1,
        dind_container_id: "c".repeat(64),
        dind_init_pid: real + 1_000_003,
        dind_init_start_ticks: 1,
        dind_helper_pid: real + 1_000_004,
        dind_helper_start_ticks: 1,
        docker_root_path: "/var/lib/docker".to_owned(),
    }
}

fn observe_with_unlinked_real_dockerd() -> TestResult<()> {
    let directory = ScopedTempDir::new("daemon-binding")?;
    let (real, real_binary) = spawn_dockerd(directory.path(), "real")?;
    let (substitute, _substitute_binary) = spawn_dockerd(directory.path(), "substitute")?;
    fs::remove_file(&real_binary)?;
    let link = fs::read_link(format!("/proc/{}/exe", real.0.id()))?;
    assert!(
        link.to_string_lossy().ends_with(" (deleted)"),
        "{}",
        link.display()
    );

    let both = observe_owned_cgroups(&request(real.0.id()));
    match &both {
        Ok(observation) => assert_eq!(
            observation.daemon.host_pid,
            real.0.id(),
            "the substitute {} was recorded as the daemon",
            substitute.0.id()
        ),
        Err(ProbeError::Invalid("selected_daemon_ambiguous")) => {}
        Err(error) => return Err(format!("the substitute was not refused: {error}").into()),
    }

    drop(substitute);
    let alone = observe_owned_cgroups(&request(real.0.id()));
    let selected =
        matches!(&alone, Err(ProbeError::Invalid(name)) if name.starts_with("selected_daemon"));
    assert!(
        !selected,
        "the unlinked real dockerd was not selected: {alone:?}"
    );
    if let Ok(observation) = alone {
        assert_eq!(observation.daemon.host_pid, real.0.id());
    }
    Ok(())
}
