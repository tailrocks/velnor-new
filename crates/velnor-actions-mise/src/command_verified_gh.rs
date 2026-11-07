// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{EnvPolicy, IsolatedCommand};
use std::{ffi::OsString, fs::File};

pub(super) struct VerifiedLaunch {
    pub(super) executable: OsString,
    pub(super) args: Vec<OsString>,
    _file: File,
}

pub(super) fn prepare(command: &IsolatedCommand) -> Result<Option<VerifiedLaunch>, String> {
    if !matches!(command.policy, EnvPolicy::GhApi | EnvPolicy::GitHubToken) {
        return Ok(None);
    }
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        prepare_linux(command).map(Some)
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = command;
        Err("verified_gh_unsupported_platform".into())
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn prepare_linux(command: &IsolatedCommand) -> Result<VerifiedLaunch, String> {
    use std::os::fd::AsRawFd as _;
    #[cfg(target_arch = "x86_64")]
    let qualified = crate::catalog::QUALIFIED_GH_LINUX_AMD64;
    #[cfg(target_arch = "aarch64")]
    let qualified = crate::catalog::QUALIFIED_GH_LINUX_ARM64;
    let invalid = || "verified_gh_invalid".to_owned();
    let pin = format!("gh@{}", qualified.version);
    let expected = [
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        pin.as_str(),
        "--",
        "gh",
    ];
    if command.program != "mise"
        || command.args.len() < expected.len()
        || !command
            .args
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
    {
        return Err(invalid());
    }
    let data_dir = std::env::var_os("MISE_DATA_DIR")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .filter(|value| !value.is_empty())
                .map(|value| std::path::PathBuf::from(value).join("mise"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|value| std::path::PathBuf::from(value).join(".local/share/mise"))
        })
        .ok_or_else(invalid)?;
    if !data_dir.is_absolute() {
        return Err(invalid());
    }
    let path = data_dir.join(format!(
        "installs/gh/{0}/gh_{0}_{1}/bin/gh",
        qualified.version, qualified.platform
    ));
    let file = sealed_executable(&path, qualified.sha256)?;
    Ok(VerifiedLaunch {
        executable: format!("/proc/{}/fd/{}", std::process::id(), file.as_raw_fd()).into(),
        args: command.args[expected.len()..].to_vec(),
        _file: file,
    })
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn sealed_executable(path: &std::path::Path, expected_sha256: &str) -> Result<File, String> {
    use rustix::fs::{MemfdFlags, Mode, SealFlags};
    use std::io::{Read as _, Write as _};
    const MAX_SIZE: u64 = 128 * 1024 * 1024;
    let invalid = || "verified_gh_invalid".to_owned();
    let source = File::open(path).map_err(|_| invalid())?;
    let metadata = source.metadata().map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SIZE {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    source
        .take(MAX_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 != metadata.len()
        || crate::cache_sources::sha256_body_hex(&bytes) != expected_sha256
    {
        return Err(invalid());
    }
    let fd = rustix::fs::memfd_create(
        "velnor-verified-gh",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )
    .map_err(|_| invalid())?;
    let mut file = File::from(fd);
    file.write_all(&bytes).map_err(|_| invalid())?;
    rustix::fs::fchmod(&file, Mode::RUSR | Mode::XUSR).map_err(|_| invalid())?;
    rustix::fs::fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .map_err(|_| invalid())?;
    Ok(file)
}

#[cfg(all(test, target_os = "macos"))]
#[test]
fn github_command_rejects_unsupported_platform() {
    let command = IsolatedCommand {
        program: "mise".into(),
        args: Vec::new(),
        cwd: None,
        extra_env: Vec::new(),
        policy: EnvPolicy::GhApi,
        operator_guarded: false,
    };
    assert!(matches!(prepare(&command), Err(error) if error == "verified_gh_unsupported_platform"));
}

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::os::fd::AsRawFd as _;

    #[test]
    fn mismatched_catalog_version_is_rejected() {
        let command = IsolatedCommand {
            program: "mise".into(),
            args: [
                "--no-config",
                "--no-env",
                "--no-hooks",
                "exec",
                "gh@0.0.0",
                "--",
                "gh",
            ]
            .into_iter()
            .map(Into::into)
            .collect(),
            cwd: None,
            extra_env: Vec::new(),
            policy: EnvPolicy::GhApi,
            operator_guarded: false,
        };
        assert!(prepare_linux(&command).is_err());
    }

    #[test]
    fn verified_snapshot_executes_unchanged_after_path_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("gh");
        std::fs::copy("/bin/true", &path)?;
        let bytes = std::fs::read(&path)?;
        let digest = crate::cache_sources::sha256_body_hex(&bytes);
        let mut snapshot = sealed_executable(&path, &digest)?;
        assert!(snapshot.write_all(b"corrupt").is_err());
        std::fs::write(&path, b"counterfeit")?;
        assert!(sealed_executable(&path, &digest).is_err());
        let verified = format!("/proc/{}/fd/{}", std::process::id(), snapshot.as_raw_fd());
        assert!(std::process::Command::new(verified).status()?.success());
        Ok(())
    }
}

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    feature = "release-git-rehearsal"
))]
#[test]
fn official_gh_runs_from_verified_snapshot() -> Result<(), Box<dyn std::error::Error>> {
    let exec = crate::PinnedToolExec::new(
        vec![crate::PinnedTool::Gh],
        std::ffi::OsStr::new("gh"),
        vec!["--version".into()],
    )?;
    let output = exec
        .command_with_github_token(&crate::ToolCatalog::pinned(), "synthetic-token")?
        .run_bounded(4096, std::time::Duration::from_secs(30))?;
    assert!(output.success);
    assert!(std::str::from_utf8(&output.stdout)?.starts_with("gh version 2.103.3"));
    Ok(())
}

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    feature = "release-git-rehearsal"
))]
#[test]
fn official_gh_retains_verified_launch_until_reaped() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read as _, Write as _};
    use std::time::Duration;

    if std::env::var_os("VELNOR_GH_LIFETIME_CHILD").is_none() {
        let status = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "command::verified_gh::official_gh_retains_verified_launch_until_reaped",
            ])
            .env("VELNOR_GH_LIFETIME_CHILD", "1")
            .status()?;
        assert!(status.success());
        return Ok(());
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let endpoint = format!("http://{}/fixture", listener.local_addr()?);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .map_err(|error| error.to_string())?;
        let mut request = [0_u8; 4096];
        if stream
            .read(&mut request)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Err("empty_request".to_owned());
        }
        ready_tx.send(()).map_err(|error| error.to_string())?;
        release_rx
            .recv_timeout(Duration::from_secs(20))
            .map_err(|error| error.to_string())?;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .map_err(|error| error.to_string())
    });
    let worker = std::thread::spawn(move || -> Result<_, crate::MiseError> {
        let exec = crate::PinnedToolExec::new(
            vec![crate::PinnedTool::Gh],
            std::ffi::OsStr::new("gh"),
            vec!["api".into(), endpoint.into()],
        )?;
        exec.command_with_github_token(&crate::ToolCatalog::pinned(), "synthetic-token")?
            .run_bounded(4096, Duration::from_secs(20))
    });
    ready_rx.recv_timeout(Duration::from_secs(20))?;
    let retained = verified_launch_fds()?;
    release_tx.send(())?;
    let output = worker.join().map_err(|_| "worker panicked")??;
    server
        .join()
        .map_err(|_| "server panicked")?
        .map_err(|_| "server failed")?;
    assert!(output.success);
    assert_eq!(retained, 1);
    assert_eq!(verified_launch_fds()?, 0);
    Ok(())
}

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64"),
    feature = "release-git-rehearsal"
))]
fn verified_launch_fds() -> Result<usize, std::io::Error> {
    let mut found = 0;
    for entry in std::fs::read_dir("/proc/self/fd")? {
        let path = entry?.path();
        if std::fs::read_link(path).is_ok_and(|target| {
            target
                .to_string_lossy()
                .contains("memfd:velnor-verified-gh")
        }) {
            found += 1;
        }
    }
    Ok(found)
}
