//! Fixed Rust package proof sources, supplied to the generic release composer.

use velnor_actions_contract::{CompiledSupportSource, ContractError};

/// Fixed exact-source Cargo metadata gate in the protected policy checkout.
pub const SOURCE_VALIDATION_PATH: &str = ".github/velnor/release_source_validation.py";

/// Emit the complete Rust-owned package proof source set.
///
/// These records carry generation bytes only. The release composer binds their
/// fixed dependency order, launchers, and interpreter authority separately.
/// # Errors
/// Rejects invalid generator markers or support source identities.
pub fn support_sources(version: &str) -> Result<Vec<CompiledSupportSource>, ContractError> {
    package_sources()
        .into_iter()
        .chain(source_intent_sources())
        .chain(publisher_sources())
        .map(|(path, source)| CompiledSupportSource::compiled(path, source, version))
        .collect()
}

fn package_sources() -> [(&'static str, &'static str); 7] {
    [
        (
            SOURCE_VALIDATION_PATH,
            include_str!("release_source_validation.py"),
        ),
        (
            ".github/velnor/release_reconcile_cargo.py",
            include_str!("release_reconcile_cargo.py"),
        ),
        (
            ".github/velnor/release_reconcile_registry.py",
            include_str!("release_reconcile_registry.py"),
        ),
        (
            ".github/velnor/release_preflight_cargo.py",
            include_str!("release_preflight_cargo.py"),
        ),
        (
            ".github/velnor/release_package.py",
            include_str!("release_package.py"),
        ),
        (
            ".github/velnor/release_publish_metadata.py",
            include_str!("release_publish_metadata.py"),
        ),
        (
            ".github/velnor/release_package_contract.py",
            include_str!("release_package_contract.py"),
        ),
    ]
}

fn source_intent_sources() -> [(&'static str, &'static str); 5] {
    [
        (
            ".github/velnor/release_source_intent_contract.py",
            include_str!("release_source_intent_contract.py"),
        ),
        (
            ".github/velnor/release_source_intent_guard.py",
            include_str!("release_source_intent_guard.py"),
        ),
        (
            ".github/velnor/release_source_intent_cargo.py",
            include_str!("release_source_intent_cargo.py"),
        ),
        (
            ".github/velnor/release_source_intent_prepare.py",
            include_str!("release_source_intent_prepare.py"),
        ),
        (
            ".github/velnor/release_source_intent_verify.py",
            include_str!("release_source_intent_verify.py"),
        ),
    ]
}

fn publisher_sources() -> [(&'static str, &'static str); 7] {
    [
        (
            ".github/velnor/release_publish_manifest.py",
            include_str!("release_publish_manifest.py"),
        ),
        (
            ".github/velnor/release_publish_transport.py",
            include_str!("release_publish_transport.py"),
        ),
        (
            ".github/velnor/release_publish_auth.py",
            include_str!("release_publish_auth.py"),
        ),
        (
            ".github/velnor/release_publish_registry.py",
            include_str!("release_publish_registry.py"),
        ),
        (
            ".github/velnor/release_publish_verify.py",
            include_str!("release_publish_verify.py"),
        ),
        (
            ".github/velnor/release_publish_artifact.py",
            include_str!("release_publish_artifact.py"),
        ),
        (
            ".github/velnor/release_publish_entry.py",
            include_str!("release_publish_entry.py"),
        ),
    ]
}
