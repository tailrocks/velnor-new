// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseObservation, GitHubAdmissionRead, IsolatedCommand, JournalIo,
    OrchestratorError, Path, UploadPermit, internal,
};
#[cfg(unix)]
use super::{Duration, validate_upload_permit};

#[cfg(unix)]
pub(super) fn execute_upload<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    command: &IsolatedCommand,
    git_root: &Path,
    socket: &Path,
    input: &CrateReleaseObservation<'_, J, G, R>,
    permit: &UploadPermit,
) -> Result<bool, OrchestratorError> {
    use std::os::unix::net::UnixListener;
    let invalid = || internal("crate_release_upload_unverified");
    #[cfg(test)]
    let timeout = std::env::var("VELNOR_NATIVE_PROVIDER_DEADLINE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(Duration::from_secs(600), Duration::from_millis);
    #[cfg(not(test))]
    let timeout = Duration::from_secs(600);
    let deadline = velnor_actions_mise::CheckDeadline::after(timeout)
        .map_err(|_| invalid())?
        .effective();
    let listener = UnixListener::bind(socket).map_err(|_| invalid())?;
    listener.set_nonblocking(true).map_err(|_| invalid())?;
    #[cfg(test)]
    let resume = if let Some(path) = std::env::var_os("VELNOR_NATIVE_PROVIDER_CONTEXT") {
        let resume = Path::new(&path).with_extension("resume");
        let provider = socket
            .parent()
            .ok_or_else(invalid)?
            .join("credential-provider");
        let context = serde_json::json!({"socket": socket, "provider": provider});
        std::fs::write(path, context.to_string()).map_err(|_| invalid())?;
        Some(resume)
    } else {
        None
    };
    velnor_actions_mise::release_git_pin::verify_bundle(git_root).map_err(|_| invalid())?;
    let result = std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel();
        scope.spawn(move || {
            #[cfg(test)]
            if let Some(resume) = resume {
                while !resume.exists() && deadline.remaining().is_ok() {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            drop(tx.send(command.run_quiescent_until(1024 * 1024, deadline)));
        });
        loop {
            if let Ok(result) = rx.try_recv() {
                break result.map_err(|_| internal("crate_release_upload_incomplete"));
            }
            let Ok(remaining) = deadline.remaining() else {
                break Err(internal("crate_release_upload_incomplete"));
            };
            match listener.accept() {
                Ok((stream, _)) => {
                    handle_provider_stream(stream, remaining, deadline, input, permit);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10).min(remaining));
                }
                Err(_) => break Err(invalid()),
            }
        }
    })?;
    velnor_actions_mise::release_git_pin::verify_bundle(git_root).map_err(|_| invalid())?;
    Ok(result.0.success)
}

#[cfg(unix)]
fn handle_provider_stream<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    mut stream: std::os::unix::net::UnixStream,
    remaining: Duration,
    deadline: velnor_actions_mise::CheckDeadline,
    input: &CrateReleaseObservation<'_, J, G, R>,
    permit: &UploadPermit,
) {
    use std::io::{BufRead, BufReader, Read, Write};
    let timeout = Some(remaining.min(Duration::from_secs(30)));
    let mut line = String::new();
    let received = stream.set_read_timeout(timeout).is_ok()
        && stream.set_write_timeout(timeout).is_ok()
        && BufReader::new(&stream)
            .take(8193)
            .read_line(&mut line)
            .is_ok()
        && line.len() <= 8192;
    let request = received
        .then(|| serde_json::from_str::<serde_json::Value>(&line))
        .and_then(Result::ok);
    #[cfg(test)]
    if let (Some(request), Some(path)) = (
        &request,
        std::env::var_os("VELNOR_NATIVE_PROVIDER_ATTEMPTS"),
    ) && let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let fields = serde_json::json!({"name": request["name"], "version": request["vers"], "checksum": request["cksum"], "registry": request["registry"]["index-url"]});
        drop(writeln!(file, "{fields}"));
    }
    let allowed = request.is_some_and(|request| {
        request["v"] == 1
            && request["kind"] == "get"
            && request["operation"] == "publish"
            && matches!(
                request["registry"]["index-url"].as_str(),
                Some(
                    "sparse+https://index.crates.io/"
                        | "https://github.com/rust-lang/crates.io-index"
                )
            )
            && request["name"] == permit.package
            && request["vers"] == permit.version
            && request["cksum"] == permit.archive_sha256
            && deadline
                .scope(|| validate_upload_permit(input, permit))
                .is_ok_and(|result| result.is_ok())
            && deadline.remaining().is_ok()
    });
    #[cfg(test)]
    if allowed
        && let Some(path) = std::env::var_os("VELNOR_NATIVE_PROVIDER_EVENT")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let event = serde_json::json!({"name": permit.package, "version": permit.version, "checksum": permit.archive_sha256});
        drop(writeln!(file, "{event}"));
    }
    drop(stream.write_all(if allowed { b"allow\n" } else { b"deny\n" }));
}

#[cfg(not(unix))]
pub(super) fn execute_upload<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    _: &IsolatedCommand,
    _: &Path,
    _: &Path,
    _: &CrateReleaseObservation<'_, J, G, R>,
    _: &UploadPermit,
) -> Result<bool, OrchestratorError> {
    Err(internal("crate_release_upload_unverified"))
}
