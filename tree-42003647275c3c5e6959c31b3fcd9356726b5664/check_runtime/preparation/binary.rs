//! Snapshot the exact platform-qualified Mise bytes before any invocation.
use std::path::Path;
use velnor_actions_contract_config::config::CheckPlatform;
use velnor_actions_mise::CheckDeadline;
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal;
use velnor_actions_workflow_steps::setup::{
    MISE_BINARY_SHA256_LINUX_X64, MISE_BINARY_SHA256_MACOS_ARM64, MISE_BINARY_SHA256_MACOS_X64,
};

/// Installed-byte bounds measured from checksum-qualified Mise 2026.9.18 archives.
/// Each bound travels with the SHA selected for that same native platform.
struct MiseBinaryPin {
    sha256: &'static str,
    max_bytes: u64,
}

fn mise_binary_pin(platform: CheckPlatform) -> MiseBinaryPin {
    let (sha256, max_bytes) = match platform {
        CheckPlatform::LinuxX64 => (MISE_BINARY_SHA256_LINUX_X64, 153_572_880),
        CheckPlatform::MacosArm64 => (MISE_BINARY_SHA256_MACOS_ARM64, 122_907_616),
        CheckPlatform::MacosX64 => (MISE_BINARY_SHA256_MACOS_X64, 149_027_840),
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
    velnor_actions_orchestrator_core::exclusive_write::write_exclusive_until(
        destination,
        &bytes,
        "qualified_mise_binary",
        || deadline_checkpoint(deadline),
    )?;
    executable_readonly(destination, deadline)
}

/// Hash the same bounded bytes subsequently written into owned storage.
fn verify_binary_bytes(bytes: &[u8], expected: &str) -> Result<(), OrchestratorError> {
    if bytes.is_empty() || velnor_actions_orchestrator_core::sha256::sha256_hex(bytes) != expected {
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
mod tests;
