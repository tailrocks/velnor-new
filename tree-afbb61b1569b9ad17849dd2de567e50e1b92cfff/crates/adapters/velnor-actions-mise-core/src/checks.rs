//! Static named-check discovery. Only Mise executes the native task graph.
mod capabilities;
mod execution;

pub use capabilities::{
    CheckCapabilityProof, ContainerObservation, ContainerProbeOutput, DockerDaemonObservation,
    OrbStackAppObservation, OrbStackObservation, PreparedContainer,
    validate_check_capability_proof, verify_check_capabilities,
};
pub(crate) mod file_read;
mod metadata;
mod qualification;
pub use execution::{DiscoveredCheck, QualifiedCheck};
pub use file_read::read_text;
pub use metadata::CheckEntryMetadata;
pub use qualification::{backend_key, config_for, fingerprint, selector};

/// Reject a check input field with the offending value attached.
pub fn invalid(field: &str, value: impl Into<String>) -> crate::MiseError {
    crate::MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.into(),
    }
}

/// Resolve a repository-relative existing path; symlink escapes fail closed.
/// # Errors
/// Rejects malformed paths, unavailable files, and paths outside the repository.
pub fn repository_path(
    root: &std::path::Path,
    relative: &str,
) -> Result<std::path::PathBuf, crate::MiseError> {
    if relative != "." {
        velnor_actions_contract::canonical::normalize_posix_path(relative)
            .map_err(|e| invalid("check_path", e.to_string()))?;
    }
    let root = root
        .canonicalize()
        .map_err(|e| invalid("repository_root", e.to_string()))?;
    let resolved = root
        .join(relative)
        .canonicalize()
        .map_err(|e| invalid("check_path", format!("{relative}:{e}")))?;
    if !resolved.starts_with(&root) {
        return Err(invalid("check_path", "escapes_repository"));
    }
    Ok(resolved)
}

/// Fixed owned Rust tool-home directory name.
pub const RUSTUP_HOME_SUFFIX: &str = "rust-home";

mod system_tools;
pub use system_tools::{
    SystemToolProof, parse_system_tool_version, validate_system_tool_proofs,
    verify_check_system_tools,
};
