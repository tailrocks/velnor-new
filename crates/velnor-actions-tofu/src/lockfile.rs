// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_contract::{ContractError, Finding, digest_b3};

use crate::family::LOCKFILE_NAME;
use crate::file_cache::FileCache;
use crate::kinds::TofuTaskKind;
use crate::parser::{MAX_DIAGNOSTIC_CHARS, parse_native};
use crate::task_identity::DigestSlot;

pub const LOCKFILE_CORRUPT: &str = "tofu_lockfile_corrupt";
pub const LOCKFILE_UNPINNED_HASHES: &str = "tofu_lockfile_unpinned_hashes";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHashCount {
    pub address: String,
    pub hashes: usize,
}

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

pub(crate) fn lock_relative(unit_path: &str) -> String {
    if unit_path == "." || unit_path.is_empty() {
        LOCKFILE_NAME.to_owned()
    } else {
        format!("{unit_path}/{LOCKFILE_NAME}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockfileSpec {
    pub providers: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LockfileInspection {
    pub file: String,
    pub spec: Option<LockfileSpec>,
    pub findings: Vec<Finding>,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TofuLockSnapshot {
    locks: Vec<(String, Option<Vec<u8>>)>,
    workdirs: Vec<(String, bool)>,
    unreadable: Vec<String>,
}

impl TofuLockSnapshot {
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

fn lock_dir(lock: &str) -> String {
    lock.strip_suffix(&format!("/{LOCKFILE_NAME}"))
        .map_or_else(String::new, str::to_owned)
}
