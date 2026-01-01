//! P08 Cargo source subset, single writer, and restore-before-fetch order.
//!
//! Sources live at the Cargo home actually used (`MISE_CARGO_HOME`,
//! `${{ runner.temp }}/velnor/cargo`), never the ambient `~/.cargo`.
//! Only the sufficient subset is archived (Cargo CI guidance):
//! `.crates.toml`, `.crates2.json`, `bin/`, `registry/index/`,
//! `registry/cache/`, `git/db/`. Extracted `registry/src/` is omitted
//! (re-extracted from cache; avoids cache/src duplication). Credentials
//! (`credentials*`, token-bearing configs) are never archived.
//!
//! One race-safe trusted writer (the plan job) seeds the shared immutable
//! snapshot; crate jobs restore read-only and never save the same key.
//! Seven jobs racing to save one immutable key is rejected by construction.

use velnor_actions_contract_workflow::StepRole;
use velnor_actions_mise_core::error::MiseError;

/// Sufficient Cargo-home subset (relative to the owned home).
pub const SOURCE_SUBSET: [&str; 6] = [
    ".crates.toml",
    ".crates2.json",
    "bin",
    "registry/index",
    "registry/cache",
    "git/db",
];

/// Role allowed to save the shared sources snapshot.
pub const TRUSTED_WRITER_ROLE: &str = "plan";

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

/// Fetch decision after a restore attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchDecision {
    /// All locked sources present: skip online fetch, run offline.
    OfflineSkip,
    /// Cold/incomplete cache: fetch via the explicit path, record miss.
    ExplicitFetch {
        /// Closed miss reason (`no_entry`, `source_missing`, ...).
        miss_reason: &'static str,
    },
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

/// True only for the single trusted writer role.
#[must_use]
pub fn is_trusted_writer(role: &str) -> bool {
    role == TRUSTED_WRITER_ROLE
}

/// Decide offline-skip vs explicit fetch from a completeness probe.
///
/// # Errors
///
/// Returns [`MiseError::InvalidStepInput`] for an unknown reason.
pub fn fetch_decision(
    sources_complete: bool,
    miss_reason: &'static str,
) -> Result<FetchDecision, MiseError> {
    if sources_complete {
        return Ok(FetchDecision::OfflineSkip);
    }
    if !matches!(
        miss_reason,
        "no_entry" | "source_missing" | "cache_unavailable" | "cache_corrupt"
    ) {
        return Err(MiseError::InvalidStepInput {
            field: "miss_reason".to_owned(),
            value: miss_reason.to_owned(),
        });
    }
    Ok(FetchDecision::ExplicitFetch { miss_reason })
}

/// Require restore/config steps before every fetch/build/test step.
///
/// `roles` is the job's typed semantic sequence. Every source fetch must
/// follow a sources restore and, on MBX jobs, the MBX objects restore.
///
/// # Errors
///
/// Returns [`MiseError::CacheNotEligible`] when fetch precedes restore.
pub fn check_restore_before_fetch(
    roles: &[Option<StepRole>],
    has_mbx: bool,
) -> Result<(), MiseError> {
    let fetches = roles
        .iter()
        .enumerate()
        .filter_map(|(index, role)| (*role == Some(StepRole::CargoSourcesFetch)).then_some(index));
    for fetch_at in fetches {
        let source_restore = roles
            .iter()
            .position(|role| *role == Some(StepRole::CargoSourcesRestore));
        if source_restore.is_some_and(|restore_at| fetch_at < restore_at) {
            return Err(MiseError::CacheNotEligible {
                task: "fetch".to_owned(),
                reason: "fetch_before_restore".to_owned(),
            });
        }
        if has_mbx {
            let mbx_restore = roles
                .iter()
                .position(|role| *role == Some(StepRole::MbxCache));
            if mbx_restore.is_some_and(|restore_at| fetch_at < restore_at) {
                return Err(MiseError::CacheNotEligible {
                    task: "fetch".to_owned(),
                    reason: "fetch_before_mbx".to_owned(),
                });
            }
        }
    }
    Ok(())
}
