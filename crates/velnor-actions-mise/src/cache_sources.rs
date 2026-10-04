//! Canonical credential-free Cargo source archive subset.
//!
//! Sources live at the Cargo home actually used (`MISE_CARGO_HOME`,
//! `${{ runner.temp }}/velnor/cargo`), never the ambient `~/.cargo`.
//! Only credential-free source state is archived: registry index/archive
//! data and Git object databases. Executables and install metadata belong
//! to the tool payload. Extracted registry sources are reconstructed from
//! archives; credentials and ambient Cargo configuration are excluded.
//!
//! Reader and pure-producer transports share these paths and validation.
//! Writer admission belongs to the typed source-producer and cache-mode gates.

use crate::error::MiseError;

/// Sufficient Cargo-home subset (relative to the owned home).
pub const SOURCE_SUBSET: [&str; 3] = ["registry/index", "registry/cache", "git/db"];

/// Never-archive markers: state, plans, and credential-bearing names
/// must never enter a cache archive (T22).
///
/// The renderer mirrors this list exactly (`cache_steps`); the
/// orchestrator pins both equal by test, like the credential
/// denylists. Scope is name markers only: other secret-shaped names
/// (secret/token/secrets substrings) stay archivable — accepted V1
/// under-inclusion (B12), contained by the fixed archive subset.
pub const NEVER_ARCHIVE_MARKERS: [&str; 3] = ["credentials", ".tfstate", ".tfplan"];

/// True when `path` names state, plans, or credentials.
#[must_use]
pub fn is_never_archive_path(path: &str) -> bool {
    NEVER_ARCHIVE_MARKERS
        .iter()
        .any(|marker| path.contains(marker))
}

/// Source-subset archive paths under one Cargo home expression.
///
/// # Errors
///
/// Returns [`MiseError::InvalidStepInput`] for a blank home.
pub fn sources_cache_paths(cargo_home: &str) -> Result<Vec<String>, MiseError> {
    if cargo_home.trim().is_empty() {
        return Err(MiseError::InvalidStepInput {
            field: "cargo_home".to_owned(),
            value: cargo_home.to_owned(),
        });
    }
    Ok(SOURCE_SUBSET
        .iter()
        .map(|suffix| format!("{cargo_home}/{suffix}"))
        .collect())
}

/// Validate subset paths: under the owned home, no credentials, no escapes.
///
/// # Errors
///
/// Returns [`MiseError::InvalidStepInput`] for absolute escapes,
/// credentials, `registry/src`, or paths outside `cargo_home`.
pub fn validate_sources_subset(paths: &[String], cargo_home: &str) -> Result<(), MiseError> {
    for path in paths {
        if path.contains("..") || is_never_archive_path(path) {
            return Err(reject(path));
        }
        if !path.starts_with(&format!("{cargo_home}/")) {
            return Err(reject(path));
        }
        let suffix = path.strip_prefix(&format!("{cargo_home}/")).unwrap_or("");
        if suffix.starts_with("registry/src") {
            return Err(reject(path));
        }
        let allowed = SOURCE_SUBSET.iter().any(|ok| {
            suffix == *ok
                || suffix.starts_with(&format!("{ok}/"))
                || *ok == "bin" && suffix == "bin"
        });
        if !allowed {
            return Err(reject(path));
        }
    }
    Ok(())
}

/// Shared rejection for a bad sources path.
fn reject(path: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "sources_path".to_owned(),
        value: path.to_owned(),
    }
}
