//! Bounded native preflight for tar inputs consumed by repository tooling.

pub mod owned_archive_guard;
pub use owned_archive_guard::{
    validate_qualification_manifest, validate_qualification_output_bindings,
};

#[path = "../build_support/archive_guard_inputs.rs"]
mod archive_guard_inputs;

/// Compute the checkout source fingerprint using the build-script algorithm.
///
/// # Errors
/// Returns an error when source admission, file identity, or input bounds fail.
pub fn source_fingerprint(root: &std::path::Path) -> Result<String, String> {
    archive_guard_inputs::fingerprint(root)
        .map(|(fingerprint, _)| fingerprint)
        .map_err(|error| format!("cannot verify archive guard source tree: {error}"))
}
