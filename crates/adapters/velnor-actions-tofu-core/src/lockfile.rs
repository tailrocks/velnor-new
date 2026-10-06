//! Root lockfile slot, lockfile inspection, and lock snapshots.
//!
//! The root lock (`.terraform.lock.hcl`, root-level per contract S10)
//! binds into identity and closure over the [`DigestSlot`] vocabulary:
//! content, proven absence, or explicit ignorance — absence and
//! ignorance never collapse. `fmt` never reads the lock, so its slot
//! is always the kind exclusion. Inspection is pure over bytes the
//! caller supplies; snapshots bracket plan and generation to prove
//! the read-only contract (no lock or workdir mutation).

use std::path::Path;

use velnor_actions_contract::{ContractError, digest_b3};
use velnor_actions_contract_release::Finding;

use crate::family::LOCKFILE_NAME;
use crate::file_cache::FileCache;
use crate::kinds::TofuTaskKind;
use crate::parser::{MAX_DIAGNOSTIC_CHARS, parse_native};
use crate::task_identity::DigestSlot;

/// Stable code for an unparseable committed lockfile.
pub const LOCKFILE_CORRUPT: &str = "tofu_lockfile_corrupt";
/// Stable code for provider blocks without `hashes`.
pub const LOCKFILE_UNPINNED_HASHES: &str = "tofu_lockfile_unpinned_hashes";

/// `hashes` entry count of one top-level `provider` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHashCount {
    /// First label (provider address; empty when unlabeled).
    pub address: String,
    /// Entries in the `hashes` array (0 when missing or not an array).
    pub hashes: usize,
}

/// Count the `hashes` entries of one native `provider` block.
///
/// Counts array entries without walking them (no budget impact):
/// only emptiness is structural; entry shapes stay tofu's runtime
/// concern. Called from the structural walk for every top-level
/// `provider` block.
pub(crate) fn provider_hash_count(block: &hcl::Block) -> ProviderHashCount {
    let address = block
        .labels()
        .first()
        .map_or_else(String::new, |label| label.as_str().to_owned());
    let mut hashes = 0;
    for attribute in block.body().attributes() {
        if attribute.key.as_str() == "hashes"
            && let hcl::Expression::Array(items) = &attribute.expr
        {
            hashes = items.len();
        }
    }
    ProviderHashCount { address, hashes }
}

/// Lockfile slot at `root`: content, proven absence, or unknown.
///
/// `unit_path` is the unit evidence path (`.` or the root
/// directory). There is no walk-up for tofu: every root binds only
/// its own lockfile.
#[must_use]
pub fn lock_digest_at_root(root: &Path, unit_path: &str, reads: &mut FileCache) -> DigestSlot {
    let relative = lock_relative(unit_path);
    match reads.read_raw(&root.join(&relative)) {
        Ok(bytes) => DigestSlot::Known(digest_b3(&bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            DigestSlot::AbsentProven(format!("not_found:{relative}"))
        }
        Err(err) => DigestSlot::Unknown(format!("unreadable:{relative}:{err}")),
    }
}

/// Lockfile slot for one task kind: the kind exclusion for `fmt`.
///
/// `fmt` never reads the lock (S2); every other kind binds the root
/// slot. Identity resolves through the root slot; closure keeps its
/// own unit-relative probe with the identical exclusion spelling
/// (its evidence strings stay byte-stable).
#[must_use]
pub fn lock_slot_for_kind(
    root: &Path,
    unit_path: &str,
    kind: TofuTaskKind,
    reads: &mut FileCache,
) -> DigestSlot {
    if kind == TofuTaskKind::Fmt {
        return DigestSlot::AbsentProven("excluded:kind_does_not_read_lockfile".to_owned());
    }
    lock_digest_at_root(root, unit_path, reads)
}

/// Repo-relative lockfile path for one unit evidence path.
pub(crate) fn lock_relative(unit_path: &str) -> String {
    if unit_path == "." || unit_path.is_empty() {
        LOCKFILE_NAME.to_owned()
    } else {
        format!("{unit_path}/{LOCKFILE_NAME}")
    }
}

/// Extracted lockfile selection: pinned provider addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockfileSpec {
    /// Provider addresses from `provider` blocks, sorted unique.
    pub providers: Vec<String>,
}

/// Outcome of one lockfile inspection.
#[derive(Debug, Clone)]
pub struct LockfileInspection {
    /// Inspected repository-relative file.
    pub file: String,
    /// Extracted selection (`None` when missing or corrupt).
    pub spec: Option<LockfileSpec>,
    /// Findings with manual guidance; empty when neutral or valid.
    pub findings: Vec<Finding>,
}

/// Inspect supplied lockfile bytes (`None` when missing).
///
/// Read-only: a corrupt lock becomes a finding with manual
/// remediation, never an error and never a repair. Provider blocks
/// without `hashes` become an unpinned finding the same way. Missing
/// and empty locks carry no claim (absence is a slot state; an empty
/// lock is neutral and ignored).
///
/// # Errors
///
/// Returns [`ContractError`] unless `path` names a lockfile.
pub fn inspect_lockfile(
    path: &str,
    content: Option<&str>,
) -> Result<LockfileInspection, ContractError> {
    if path.rsplit('/').next().unwrap_or(path) != LOCKFILE_NAME {
        return Err(ContractError::identity(
            "lockfile",
            format!("not_owned:{path}"),
        ));
    }
    let file = path.to_owned();
    let Some(text) = content else {
        return Ok(LockfileInspection {
            file,
            spec: None,
            findings: Vec::new(),
        });
    };
    if text.trim().is_empty() {
        return Ok(LockfileInspection {
            file,
            spec: Some(LockfileSpec {
                providers: Vec::new(),
            }),
            findings: Vec::new(),
        });
    }
    match parse_native(text) {
        Ok(model) => {
            let unpinned: Vec<String> = model
                .provider_hash_counts
                .iter()
                .filter(|entry| entry.hashes == 0)
                .map(|entry| entry.address.clone())
                .collect();
            let findings = if unpinned.is_empty() {
                Vec::new()
            } else {
                vec![unpinned_hashes_finding(path, &unpinned)]
            };
            Ok(LockfileInspection {
                file,
                spec: Some(LockfileSpec {
                    providers: provider_addresses(&model),
                }),
                findings,
            })
        }
        Err(problem) => Ok(LockfileInspection {
            file,
            spec: None,
            findings: vec![corrupt_lockfile_finding(path, &problem.to_string())],
        }),
    }
}

/// Sorted unique provider addresses of one parsed lockfile.
fn provider_addresses(model: &crate::parser::FileModel) -> Vec<String> {
    let mut providers: Vec<String> = model
        .blocks
        .iter()
        .filter(|block| block.kind == "provider")
        .filter_map(|block| block.labels.first().cloned())
        .filter(|address| !address.is_empty())
        .collect();
    providers.sort();
    providers.dedup();
    providers
}

/// Unpinned-hashes finding naming the providers without them (capped).
fn unpinned_hashes_finding(path: &str, providers: &[String]) -> Finding {
    let observed: String = providers
        .join(",")
        .chars()
        .take(MAX_DIAGNOSTIC_CHARS)
        .collect();
    Finding {
        code: LOCKFILE_UNPINNED_HASHES.to_owned(),
        path: path.to_owned(),
        observed: Some(observed),
        recommended: None,
        action: Some(
            "restore the missing `hashes` by running `tofu providers lock` manually and \
             commit the result; Velnor never repairs the lock"
                .to_owned(),
        ),
        reason: "provider entries without hashes let init resolve outside the committed \
             pins; validate cannot prove the provider set"
            .to_owned(),
    }
}

/// Corrupt-lock finding: manual regeneration, never a repair.
pub(crate) fn corrupt_lockfile_finding(path: &str, problem: &str) -> Finding {
    Finding {
        code: LOCKFILE_CORRUPT.to_owned(),
        path: path.to_owned(),
        observed: Some(format!("unparseable_lockfile:{problem}")),
        recommended: None,
        action: Some(
            "regenerate the lockfile manually with `tofu providers lock` and commit it; \
             Velnor never repairs or deletes the lock"
                .to_owned(),
        ),
        reason: "a corrupt lockfile blocks validate; only a human repair unblocks it".to_owned(),
    }
}

/// Byte snapshot of root lockfiles plus workdir presence.
///
/// Captured before planning or generation and verified after, so a
/// lockfile write or a `.terraform` workdir appearing mid-run fails
/// closed (contract §4.5: no lock mutation, no init during
/// discovery).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TofuLockSnapshot {
    /// One entry per root lock: bytes, or `None` when absent.
    locks: Vec<(String, Option<Vec<u8>>)>,
    /// One entry per root workdir: presence of `.terraform`.
    workdirs: Vec<(String, bool)>,
    /// Locks present but unreadable: never merged with missing.
    unreadable: Vec<String>,
}

impl TofuLockSnapshot {
    /// Capture the lock bytes and workdir presence for `roots`.
    ///
    /// `roots` are normalized configured roots (`""` for the
    /// repository root). Symlinked locks refuse without reading and
    /// count as unreadable, like the cached-read path.
    #[must_use]
    pub fn capture(root: &Path, roots: &[String]) -> Self {
        let mut unreadable = Vec::new();
        let mut locks = Vec::with_capacity(roots.len());
        let mut workdirs = Vec::with_capacity(roots.len());
        for normalized in roots {
            let lock = lock_relative(if normalized.is_empty() {
                "."
            } else {
                normalized
            });
            let linked = std::fs::symlink_metadata(root.join(&lock))
                .is_ok_and(|meta| meta.file_type().is_symlink());
            let bytes = if linked {
                unreadable.push(lock.clone());
                None
            } else {
                match std::fs::read(root.join(&lock)) {
                    Ok(bytes) => Some(bytes),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                    Err(_) => {
                        unreadable.push(lock.clone());
                        None
                    }
                }
            };
            locks.push((lock, bytes));
            let workdir = if normalized.is_empty() {
                ".terraform".to_owned()
            } else {
                format!("{normalized}/.terraform")
            };
            workdirs.push((
                workdir.clone(),
                std::fs::symlink_metadata(root.join(&workdir)).is_ok(),
            ));
        }
        Self {
            locks,
            workdirs,
            unreadable,
        }
    }

    /// Fail when any lock or workdir differs from the captured bytes.
    ///
    /// Unreadable locks fail closed: the no-write proof needs
    /// contents, so absence and inaccessibility never merge.
    ///
    /// # Errors
    ///
    /// Returns the first drifted lock or workdir reason.
    pub fn verify(&self, root: &Path) -> Result<(), String> {
        let roots: Vec<String> = self.locks.iter().map(|(lock, _)| lock_dir(lock)).collect();
        let fresh = Self::capture(root, &roots);
        let mut bad = self.unreadable.clone();
        bad.extend(fresh.unreadable.iter().cloned());
        if let Some(first) = bad.iter().min() {
            return Err(format!("tofu_lock_unreadable:{first}"));
        }
        for ((rel, want), (_, got)) in self.locks.iter().zip(fresh.locks.iter()) {
            if want != got {
                return Err(format!("tofu_lock_changed:{rel}"));
            }
        }
        for ((rel, want), (_, got)) in self.workdirs.iter().zip(fresh.workdirs.iter()) {
            if want != got {
                return Err(format!("tofu_terraform_dir_changed:{rel}"));
            }
        }
        Ok(())
    }
}

/// Normalized root backing one captured lock path.
fn lock_dir(lock: &str) -> String {
    lock.strip_suffix(&format!("/{LOCKFILE_NAME}"))
        .map_or_else(String::new, str::to_owned)
}
