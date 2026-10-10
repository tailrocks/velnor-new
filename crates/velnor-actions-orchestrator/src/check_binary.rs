//! Snapshot the exact platform-pinned Mise bytes before any invocation.
use crate::OrchestratorError;
use crate::internal::internal;
use std::path::Path;
use velnor_actions_contract::config::CheckPlatform;
use velnor_actions_mise::CheckDeadline;
use velnor_actions_workflow_renderer::setup::{
    MISE_BINARY_SHA256_LINUX_X64, MISE_BINARY_SHA256_MACOS_ARM64, MISE_BINARY_SHA256_MACOS_X64,
};

/// Raw executable byte counts measured for checksum-qualified Mise 2026.10.7.
/// Each bound travels with the SHA selected for that same native platform.
struct MiseBinaryPin {
    sha256: &'static str,
    max_bytes: u64,
}

fn mise_binary_pin(platform: CheckPlatform) -> MiseBinaryPin {
    let (sha256, max_bytes) = match platform {
        CheckPlatform::LinuxX64 => (MISE_BINARY_SHA256_LINUX_X64, 161_070_528),
        CheckPlatform::MacosArm64 => (MISE_BINARY_SHA256_MACOS_ARM64, 127_277_984),
        CheckPlatform::MacosX64 => (MISE_BINARY_SHA256_MACOS_X64, 155_846_176),
    };
    MiseBinaryPin { sha256, max_bytes }
}

/// Ambient PATH only locates bytes; it never authorizes executable identity.
pub(super) fn project_mise_binary(
    home: &Path,
    platform: CheckPlatform,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let source = velnor_actions_mise::command::resolve_check_program("mise")
        .map_err(|e| internal(&e.to_string()))?;
    let pin = mise_binary_pin(platform);
    project_binary_until(
        Path::new(&source),
        &home.join("bin/mise"),
        pin.sha256,
        pin.max_bytes,
        Some(deadline),
    )
}

#[cfg(test)]
fn project_binary(
    source: &Path,
    destination: &Path,
    expected: &str,
    max_bytes: u64,
) -> Result<(), OrchestratorError> {
    project_binary_until(source, destination, expected, max_bytes, None)
}

fn project_binary_until(
    source: &Path,
    destination: &Path,
    expected: &str,
    max_bytes: u64,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    let bytes =
        crate::retrieve_reports::staged_reads::read_staged_bytes_until(source, max_bytes, || {
            read_deadline_checkpoint(deadline)
        })
        .map_err(|_| internal("mise_binary_unreadable"))?;
    verify_binary_bytes(&bytes, expected)?;
    crate::exclusive_write::write_exclusive_until(
        destination,
        &bytes,
        "qualified_mise_binary",
        || deadline_checkpoint(deadline),
    )?;
    executable_readonly(destination, deadline)
}

/// Hash the same bounded bytes subsequently written into owned storage.
fn verify_binary_bytes(bytes: &[u8], expected: &str) -> Result<(), OrchestratorError> {
    if bytes.is_empty() || crate::cover_identity::generator::sha256_hex(bytes) != expected {
        return Err(internal("mise_binary_unqualified_sha256"));
    }
    Ok(())
}

/// Set executable/read-only owner mode through a no-follow file handle.
fn executable_readonly(
    path: &Path,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    deadline_checkpoint(deadline)?;
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("mise_binary_permissions"))?;
    rustix::fs::fchmod(&fd, rustix::fs::Mode::RUSR | rustix::fs::Mode::XUSR)
        .map_err(|_| internal("mise_binary_permissions"))?;
    deadline_checkpoint(deadline)
}

fn deadline_checkpoint(deadline: Option<CheckDeadline>) -> Result<(), OrchestratorError> {
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| internal(&error.to_string()))?;
    }
    Ok(())
}

fn read_deadline_checkpoint(deadline: Option<CheckDeadline>) -> Result<(), &'static str> {
    deadline.map_or(Ok(()), |value| {
        value
            .remaining()
            .map(|_| ())
            .map_err(|_| "deadline_exhausted")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrong_digest_refuses_wrapper_without_invocation_or_copy() {
        let temp = tempfile::TempDir::new().expect("temp");
        let marker = temp.path().join("wrapper-ran");
        let source = temp.path().join("mise");
        let script = format!(
            "#!/bin/sh\nprintf invoked > '{}'\nprintf '2026.10.7\\n'\n",
            marker.display()
        );
        std::fs::write(&source, script).expect("wrapper");
        let destination = temp.path().join("owned-mise");
        let error = project_binary(
            &source,
            &destination,
            MISE_BINARY_SHA256_MACOS_ARM64,
            mise_binary_pin(CheckPlatform::MacosArm64).max_bytes,
        )
        .expect_err("unqualified");
        assert!(error.to_string().contains("mise_binary_unqualified_sha256"));
        assert!(!destination.exists());
        assert!(!marker.exists(), "unqualified bytes are never executed");
    }
    #[test]
    fn qualified_fixture_bytes_copy_exclusively_and_survive_source_replacement() {
        let temp = tempfile::TempDir::new().expect("temp");
        let source = temp.path().join("source");
        let destination = temp.path().join("owned");
        let bytes = b"qualified fixture bytes";
        let expected = crate::cover_identity::generator::sha256_hex(bytes);
        verify_binary_bytes(bytes, &expected).expect("positive identity");
        std::fs::write(&source, bytes).expect("source");
        project_binary(&source, &destination, &expected, 1024).expect("projection");
        std::fs::write(&source, b"replacement").expect("replace ambient source");
        assert_eq!(std::fs::read(&destination).expect("owned bytes"), bytes);
        assert!(
            !std::fs::symlink_metadata(&destination)
                .expect("meta")
                .file_type()
                .is_symlink()
        );
        assert!(project_binary(&source, &destination, &expected, 1024).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&destination)
                    .expect("mode")
                    .permissions()
                    .mode()
                    & 0o777,
                0o500
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn source_symlink_and_existing_destination_refuse() {
        let temp = tempfile::TempDir::new().expect("temp");
        let bytes = b"fixture";
        let expected = crate::cover_identity::generator::sha256_hex(bytes);
        let source = temp.path().join("source");
        std::fs::write(&source, bytes).expect("source");
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&source, &link).expect("link");
        assert!(project_binary(&link, &temp.path().join("owned"), &expected, 1024).is_err());
        let destination = temp.path().join("existing");
        std::fs::write(&destination, b"original").expect("existing");
        assert!(project_binary(&source, &destination, &expected, 1024).is_err());
        assert_eq!(std::fs::read(destination).expect("unchanged"), b"original");
    }
}

#[cfg(test)]
#[path = "check_binary_budget_tests.rs"]
mod budget_tests;
