//! Admit every projected runtime socket against the executing target's Unix ABI.
use super::{RuntimeEntryEvidence, RuntimeEntryKind};
use crate::OrchestratorError;
use crate::internal::internal;
use std::path::Path;

pub(super) fn validate(
    home: &Path,
    entries: &[RuntimeEntryEvidence],
) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::net::SocketAddr;
        let runtime = home.join(".orbstack/run");
        for entry in entries {
            if entry.kind != RuntimeEntryKind::UnixSocket {
                continue;
            }
            let path = runtime.join(&entry.path);
            // std checks raw OS bytes plus the terminating NUL against this
            // target's sockaddr_un.sun_path, without connecting or resolving links.
            SocketAddr::from_pathname(&path).map_err(|_| {
                let bytes = path.as_os_str().as_bytes();
                internal(&format!(
                    "runtime_socket_path_too_long: projected socket \"{}\" is {} OS bytes; shorten RUNNER_TEMP so all owned runtime socket paths fit the target Unix address capacity",
                    bytes.escape_ascii(), bytes.len()
                ))
            })?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (home, entries);
        Err(internal("container_runtime:unix_platform_required"))
    }
}

#[cfg(test)]
mod tests;
