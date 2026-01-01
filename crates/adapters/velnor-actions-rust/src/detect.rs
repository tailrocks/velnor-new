//! Rust detector: `Cargo.toml` candidates, registry order, ignore-after.
//!
//! Detectors observe the post-exclusion index; stack ignores apply after
//! detection completes and never suppress malformed-manifest errors.

use velnor_actions_contract_config::VelnorConfig;
use velnor_actions_contract_planning::{DetectedProject, FileIndex, StackCandidate};

/// One discovered `Cargo.toml` manifest (post-exclusion).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CargoCandidate {
    /// Repository-relative POSIX path of the manifest.
    pub manifest: String,
}

/// Discover every `Cargo.toml` marker exactly once, in sorted order.
#[must_use]
pub fn discover_candidates(index: &FileIndex) -> Vec<CargoCandidate> {
    index
        .files()
        .iter()
        .filter(|path| is_manifest(path))
        .map(|path| CargoCandidate {
            manifest: path.clone(),
        })
        .collect()
}

/// Whether `path` is a `Cargo.toml` marker.
fn is_manifest(path: &str) -> bool {
    path == "Cargo.toml" || path.ends_with("/Cargo.toml")
}

/// Directory holding `manifest`; empty for the repository root.
#[must_use]
pub fn project_root_for_manifest(manifest: &str) -> String {
    manifest
        .rsplit_once('/')
        .map_or_else(String::new, |(dir, _)| dir.to_owned())
}

/// Discover stack candidates: the rust detector entry.
#[must_use]
pub fn discover_stack_candidates(index: &FileIndex) -> Vec<StackCandidate> {
    debug_assert!(
        VelnorConfig::REGISTERED_STACKS.contains(&crate::STACK_ID),
        "detector emits only registered stacks"
    );
    discover_candidates(index)
        .iter()
        .map(|candidate| StackCandidate {
            stack_id: crate::STACK_ID.to_owned(),
            unit_root: project_root_for_manifest(&candidate.manifest),
        })
        .collect()
}

/// Lift rust unit candidates to detector records in stable order.
///
/// The caller partitions candidates by stack; every candidate here is
/// the rust detector's own output.
#[must_use]
pub fn detected_projects_for_units(candidates: &[StackCandidate]) -> Vec<DetectedProject> {
    candidates
        .iter()
        .map(|candidate| DetectedProject {
            stack_id: crate::STACK_ID.to_owned(),
            project_root: candidate.unit_root.clone(),
            manifest: manifest_for_unit_root(&candidate.unit_root),
        })
        .collect()
}

/// Manifest path for a manifest key.
///
/// Single owner of the key mapping: the root key names the root
/// manifest, every other key its own directory manifest.
#[must_use]
pub fn manifest_for_key(key: &str) -> String {
    if key == "root" {
        "Cargo.toml".to_owned()
    } else {
        format!("{key}/Cargo.toml")
    }
}

/// Manifest path for a rust unit root directory.
#[must_use]
pub fn manifest_for_unit_root(unit_root: &str) -> String {
    if unit_root.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{unit_root}/Cargo.toml")
    }
}
