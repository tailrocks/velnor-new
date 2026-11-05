// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{fs, io::Read as _, path::Path, process::Command, time::Duration};
use velnor_actions_mise::release_git_pin::{
    GIT_PACKAGE_SHA256, GIT_PACKAGE_URL, VerifiedGit, verify_bundle,
};

pub(crate) fn acquire(
    runner_temp: &Path,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<VerifiedGit, OrchestratorError> {
    let invalid = || internal("release_git_bundle_invalid");
    let velnor = runner_temp.join("velnor");
    if !runner_temp.is_absolute()
        || runner_temp.canonicalize().map_err(|_| invalid())? != runner_temp
        || velnor.canonicalize().map_err(|_| invalid())? != velnor
        || !velnor.is_dir()
    {
        return Err(invalid());
    }
    let root = velnor.join("git-bundle");
    if root.exists() {
        return ready(&root, deadline);
    }
    let scratch = tempfile::Builder::new()
        .prefix("git-acquire-")
        .tempdir_in(&velnor)
        .map_err(|_| invalid())?;
    let bytes = download_package(GIT_PACKAGE_URL, deadline)?;
    verify_package(&bytes, GIT_PACKAGE_SHA256)?;
    let package = scratch.path().join("git.deb");
    fs::write(&package, bytes).map_err(|_| invalid())?;
    let extracted = scratch.path().join("extracted");
    let mut extract = Command::new("/usr/bin/dpkg-deb");
    extract.args(["-x"]).arg(&package).arg(&extracted);
    if !run_probe(&mut extract, deadline)?.success {
        return Err(invalid());
    }
    let staged = scratch.path().join("bundle");
    fs::create_dir(&staged).map_err(|_| invalid())?;
    fs::copy(extracted.join("usr/bin/git"), staged.join("git")).map_err(|_| invalid())?;
    fs::copy(
        extracted.join("usr/lib/git-core/git-remote-http"),
        staged.join("git-remote-http"),
    )
    .map_err(|_| invalid())?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("git-remote-http", staged.join("git-remote-https"))
        .map_err(|_| invalid())?;
    ready(&staged, deadline)?;
    install_noreplace(&staged, &root)?;
    ready(&root, deadline)
}

fn download_package(
    url: &str,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let invalid = || internal("release_git_bundle_invalid");
    let mut response = crate::api::github_evidence_http::agent()?
        .get(url)
        .config()
        .timeout_global(Some(
            deadline
                .remaining()
                .map_err(|_| invalid())?
                .min(Duration::from_secs(60)),
        ))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .call()
        .map_err(|_| invalid())?;
    if response.status().as_u16() != 200 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    deadline.remaining().map_err(|_| invalid())?;
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn install_noreplace(staged: &Path, root: &Path) -> Result<(), OrchestratorError> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        staged,
        rustix::fs::CWD,
        root,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|_| internal("release_git_bundle_invalid"))
}

#[cfg(not(target_os = "linux"))]
fn install_noreplace(_: &Path, _: &Path) -> Result<(), OrchestratorError> {
    Err(internal("release_git_bundle_invalid"))
}

fn verify_package(bytes: &[u8], expected: &str) -> Result<(), OrchestratorError> {
    if bytes.len() > 8 * 1024 * 1024 || crate::generator::sha256_hex(bytes) != expected {
        return Err(internal("release_git_bundle_invalid"));
    }
    Ok(())
}

fn run_probe(
    command: &mut Command,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<velnor_actions_mise::command::ProcessOutput, OrchestratorError> {
    command.env_clear();
    velnor_actions_mise::command::run_prepared_quiescent_until(command, 1024 * 1024, deadline)
        .map(|(output, _)| output)
        .map_err(|error| map_probe_error(&error))
}

fn map_probe_error(error: &velnor_actions_mise::MiseError) -> OrchestratorError {
    if velnor_actions_mise::command::is_cleanup_failure(error) {
        internal("release_git_bundle_cleanup_uncertain")
    } else {
        internal("release_git_bundle_invalid")
    }
}

fn ready(
    root: &Path,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<VerifiedGit, OrchestratorError> {
    let invalid = || internal("release_git_bundle_invalid");
    let git = verify_bundle(root).map_err(|_| invalid())?;
    let mut version = Command::new(git.executable());
    version.arg("--version");
    let result = run_probe(&mut version, deadline)?;
    if !result.success || result.stdout != b"git version 2.43.0\n" {
        return Err(invalid());
    }
    let mut helper = Command::new(root.join("git-remote-http"));
    let result = run_probe(&mut helper, deadline)?;
    if result.code.is_none_or(|code| code == 127) {
        return Err(invalid());
    }
    let verified = verify_bundle(root).map_err(|_| invalid())?;
    deadline.remaining().map_err(|_| invalid())?;
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::verify_package;
    #[cfg(unix)]
    #[test]
    fn git_bundle_commands_share_deadline_and_quiesce() -> Result<(), Box<dyn std::error::Error>> {
        use std::time::{Duration, Instant};
        let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_millis(800))?;
        let started = Instant::now();
        let mut first = std::process::Command::new("/bin/sh");
        first.args(["-c", "sleep 0.35"]);
        assert!(super::run_probe(&mut first, deadline)?.success);
        let mut stalled = std::process::Command::new("/bin/sh");
        stalled.args(["-c", "sleep 3"]);
        assert!(super::run_probe(&mut stalled, deadline).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    #[test]
    fn stalled_download_body_obeys_deadline() -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read as _, Write as _};
        use std::time::{Duration, Instant};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}/git.deb", listener.local_addr()?);
        let (release, wait) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || -> std::io::Result<()> {
            let (mut socket, _) = listener.accept()?;
            let mut request = [0_u8; 4096];
            if socket.read(&mut request)? == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nx")?;
            if wait.recv_timeout(Duration::from_secs(5)).is_err() {
                return Ok(());
            }
            Ok(())
        });
        let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_millis(500))?;
        let started = Instant::now();
        let result = super::download_package(&url, deadline);
        let elapsed = started.elapsed();
        let released = release.send(()).is_ok();
        server.join().map_err(|_| "server panicked")??;
        assert!(
            elapsed < Duration::from_secs(3),
            "download ignored its deadline"
        );
        assert!(released);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn uncertain_cleanup_is_not_reported_as_normal_failure() {
        let error = velnor_actions_mise::MiseError::SpawnFailed {
            program: "fixture".into(),
            message: "failed;cleanup_failed:uncertain".into(),
        };
        assert!(
            matches!(super::map_probe_error(&error), crate::OrchestratorError::Internal { problem } if problem == "release_git_bundle_cleanup_uncertain")
        );
    }

    #[test]
    fn rejects_changed_or_oversized_package_before_extraction() {
        let bytes = b"fixture-package";
        let sha = crate::generator::sha256_hex(bytes);
        assert!(verify_package(bytes, &sha).is_ok());
        assert!(verify_package(b"fixture-tampered", &sha).is_err());
        let large = vec![0; 8 * 1024 * 1024 + 1];
        assert!(verify_package(&large, &crate::generator::sha256_hex(&large)).is_err());
    }
}
