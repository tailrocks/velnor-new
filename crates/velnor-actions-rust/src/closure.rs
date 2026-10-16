//! Rust input closures: complete first-party inputs, explicit unknowns.
//!
//! Unknown inputs forbid reuse and coverage. The kind-gated classes,
//! Cargo filenames, and Rust source extensions here are Rust-domain
//! rules; the orchestrator dispatches per stack to this resolver.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::{
    ClosureBuilder, ContractError, ProposedTask, Provenance, TaskInputClosure, digest_b3,
};

use super::closure_probes::{
    probe_cargo_config, probe_declared, probe_file, probe_lockfile, probe_nextest_config,
    source_tree_files,
};
use crate::task_identity::DigestSlot;
use crate::tasks::TaskKind;

/// Per-analysis cache of Rust checkout probes to avoid repeated directory walks and file hashing.
#[derive(Debug, Clone, Default)]
pub struct RustClosureCache {
    source_trees: BTreeMap<String, Provenance>,
    files: BTreeMap<String, Provenance>,
    lockfiles: BTreeMap<String, Provenance>,
    nextest_configs: BTreeMap<Option<String>, Provenance>,
    cargo_configs: BTreeMap<String, Provenance>,
    classes: BTreeMap<(String, u8), Provenance>,
}

impl RustClosureCache {
    /// Create an empty probe cache for one analysis pass.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Resolve one proposed task's closure against the checkout at `root`.
///
/// Reads adapter facts from the proposal (unit path, features, target,
/// configuration, drivers, kind, declared inputs); filesystem probes
/// bind content, proven absence, or explicit unknowns.
///
/// # Errors
///
/// Returns [`ContractError`] for task-kind spellings outside the seven
/// known kinds.
pub fn resolve_closure_at_root(
    root: &Path,
    task: &ProposedTask,
    profile_nextest_config: Option<&str>,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
) -> Result<TaskInputClosure, ContractError> {
    let mut cache = RustClosureCache::new();
    resolve_closure_at_root_cached(
        root,
        task,
        profile_nextest_config,
        graph_digest,
        toolchain_id,
        platform_id,
        &mut cache,
    )
}

/// Resolve one proposed task's closure using a shared [`RustClosureCache`].
///
/// # Errors
///
/// Returns [`ContractError`] for task-kind spellings outside the seven
/// known kinds.
pub fn resolve_closure_at_root_cached(
    root: &Path,
    task: &ProposedTask,
    profile_nextest_config: Option<&str>,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
    cache: &mut RustClosureCache,
) -> Result<TaskInputClosure, ContractError> {
    let kind = TaskKind::parse(&task.task_kind)?;
    let manifest = task.identity.unit_path.as_str();
    let nextest = cache
        .nextest_configs
        .entry(profile_nextest_config.map(str::to_owned))
        .or_insert_with(|| probe_nextest_config(root, profile_nextest_config))
        .clone();
    let docs = class_provenance_cached(root, kind, manifest, Class::Docs, cache);
    let fixtures = class_provenance_cached(root, kind, manifest, Class::Fixtures, cache);
    let schemas = class_provenance_cached(root, kind, manifest, Class::Schemas, cache);
    let source_tree = cache
        .source_trees
        .entry(manifest.to_owned())
        .or_insert_with(|| source_tree_provenance(root, manifest))
        .clone();
    let manifest_prov = cache
        .files
        .entry(manifest.to_owned())
        .or_insert_with(|| probe_file(root, manifest))
        .clone();
    let lockfile = cache
        .lockfiles
        .entry(manifest.to_owned())
        .or_insert_with(|| probe_lockfile(root, manifest))
        .clone();
    let cargo_config = cache
        .cargo_configs
        .entry(manifest.to_owned())
        .or_insert_with(|| probe_cargo_config(root, manifest))
        .clone();
    let mut closure = ClosureBuilder::new()
        .input("source_tree", source_tree)
        .input("manifest", manifest_prov)
        .input("lockfile", lockfile)
        .input("nextest_config", nextest)
        .input("cargo_config", cargo_config)
        .input("docs", docs)
        .input("fixtures", fixtures)
        .input("schemas", schemas)
        .digest("local_deps", graph_digest)
        .digest("toolchain", toolchain_id)
        .digest("platform", platform_id)
        .value("features", &task.identity.features.join(","))
        .value("target", &task.identity.target)
        .value("profile", &task.configuration)
        .value("driver", &task.identity.compile_driver)
        .value("runner", &task.identity.test_runner)
        .value("kind", &task.task_kind);
    for (index, extra) in task.identity.declared_inputs.iter().enumerate() {
        let name = format!("declared_extra:{index}:{extra}");
        closure = closure.input(&name, probe_declared(root, extra));
    }
    let closure = closure.input("vcs", vcs_provenance(task.identity.undeclared_reads));
    Ok(closure.build(&task.task_id))
}

/// Digest slot of a [`Provenance`], preserving absence distinctly.
///
/// Externally guarded inputs have no producer today; if one ever
/// appears it maps to unknown (fail-closed: unverified by us).
fn digest_slot(provenance: Provenance) -> DigestSlot {
    match provenance {
        Provenance::Known { digest } => DigestSlot::Known(digest),
        Provenance::AbsentProven { evidence } => DigestSlot::AbsentProven(evidence),
        Provenance::Unknown { reason } => DigestSlot::Unknown(reason),
        Provenance::GuardedExternally { guard } => {
            DigestSlot::Unknown(format!("guarded_externally:{guard}"))
        }
    }
}

/// Lockfile slot at `root`: content, proven absence, or unknown.
#[must_use]
pub fn lock_digest_at_root(root: &Path, manifest: &str) -> DigestSlot {
    digest_slot(probe_lockfile(root, manifest))
}

/// Cached lockfile slot at `root`.
#[must_use]
pub fn lock_digest_cached(root: &Path, manifest: &str, cache: &mut RustClosureCache) -> DigestSlot {
    let prov = cache
        .lockfiles
        .entry(manifest.to_owned())
        .or_insert_with(|| probe_lockfile(root, manifest))
        .clone();
    digest_slot(prov)
}

/// Nextest-config slot at `root`: content, proven absence, or unknown.
#[must_use]
pub fn nextest_digest_at_root(root: &Path, profile_config: Option<&str>) -> DigestSlot {
    digest_slot(probe_nextest_config(root, profile_config))
}

/// Cached Nextest-config slot at `root`.
#[must_use]
pub fn nextest_digest_cached(
    root: &Path,
    profile_config: Option<&str>,
    cache: &mut RustClosureCache,
) -> DigestSlot {
    let prov = cache
        .nextest_configs
        .entry(profile_config.map(str::to_owned))
        .or_insert_with(|| probe_nextest_config(root, profile_config))
        .clone();
    digest_slot(prov)
}

/// VCS provenance: unknown with undeclared reads, proven absent without.
fn vcs_provenance(undeclared_reads: bool) -> Provenance {
    if undeclared_reads {
        Provenance::Unknown {
            reason: "build_script_may_observe_vcs".to_owned(),
        }
    } else {
        Provenance::AbsentProven {
            evidence: "offline_cargo_no_vcs_reads".to_owned(),
        }
    }
}

/// Source-tree provenance: digest over package `*.rs` plus the manifest.
///
/// A source edit flips the closure digest; an empty tree is unknown.
fn source_tree_provenance(root: &Path, manifest: &str) -> Provenance {
    let mut files = match source_tree_files(root, manifest) {
        Ok(files) => files,
        Err(reason) => {
            return Provenance::Unknown { reason };
        }
    };
    if let Ok((normalized, bytes)) = read_manifest_bytes(root, manifest) {
        files.push((normalized, digest_b3(&bytes)));
        files.sort();
    }
    if files.is_empty() {
        return Provenance::Unknown {
            reason: "no_source_files".to_owned(),
        };
    }
    Provenance::Known {
        digest: files_digest(&files),
    }
}

/// Normalized manifest path plus bytes for the source-tree digest.
fn read_manifest_bytes(root: &Path, manifest: &str) -> Result<(String, Vec<u8>), String> {
    let normalized =
        crate::identity::normalize_identity_path(manifest).map_err(|err| err.to_string())?;
    let bytes = std::fs::read(root.join(&normalized)).map_err(|err| err.to_string())?;
    Ok((normalized, bytes))
}

/// Digest over sorted `(path, digest)` file pairs.
fn files_digest(files: &[(String, String)]) -> String {
    let pairs: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, digest)| (path.as_str(), digest.as_str()))
        .collect();
    canonical_digest(&pairs).unwrap_or_else(|_| digest_b3(b"files_error"))
}

/// BLAKE3 digest over canonical JSON bytes.
fn canonical_digest<T: serde::Serialize>(value: &T) -> Result<String, ContractError> {
    Ok(digest_b3(&velnor_actions_contract::canonical_json_bytes(
        value,
    )?))
}

/// Conventional input class with kind-gated collection.
#[derive(Clone, Copy)]
enum Class {
    Docs,
    Fixtures,
    Schemas,
}

impl Class {
    /// Exclusion evidence when `kind` never consumes this class.
    fn excluded(self, kind: TaskKind) -> Option<&'static str> {
        match self {
            Self::Docs if !matches!(kind, TaskKind::Doc | TaskKind::Doctest) => {
                Some("excluded:kind_does_not_render_docs")
            }
            Self::Fixtures
                if !matches!(kind, TaskKind::Test | TaskKind::Nextest | TaskKind::Doctest) =>
            {
                Some("excluded:kind_does_not_execute_tests")
            }
            Self::Schemas if kind == TaskKind::Fmt => Some("excluded:kind_does_not_compile"),
            _ => None,
        }
    }

    /// True for paths this class collects.
    fn keep(self, path: &str) -> bool {
        match self {
            Self::Docs => {
                super::closure_probes::ext_is(path, "md")
                    || path.contains("/docs/")
                    || path.ends_with("README")
            }
            Self::Fixtures => {
                path.contains("fixtures/")
                    || path.contains("tests/") && super::closure_probes::ext_is(path, "json")
            }
            Self::Schemas => {
                path.contains("schemas/")
                    || super::closure_probes::SCHEMA_EXTS
                        .iter()
                        .any(|want| super::closure_probes::ext_is(path, want))
            }
        }
    }
}

/// Class provenance: exclusion for non-consuming kinds, else cached digest or absence.
fn class_provenance_cached(
    root: &Path,
    kind: TaskKind,
    manifest: &str,
    class: Class,
    cache: &mut RustClosureCache,
) -> Provenance {
    if let Some(evidence) = class.excluded(kind) {
        return Provenance::AbsentProven {
            evidence: evidence.to_owned(),
        };
    }
    cache
        .classes
        .entry((manifest.to_owned(), class as u8))
        .or_insert_with(|| {
            let keep = |_: &Path, path: &str| class.keep(path);
            match super::closure_probes::walk_package_files(root, manifest, &keep) {
                Err(reason) => Provenance::Unknown { reason },
                Ok(files) if files.is_empty() => Provenance::AbsentProven {
                    evidence: "probed_no_class_files".to_owned(),
                },
                Ok(files) => Provenance::Known {
                    digest: files_digest(&files),
                },
            }
        })
        .clone()
}
