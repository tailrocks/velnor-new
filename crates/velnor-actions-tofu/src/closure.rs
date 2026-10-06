//! Tofu input closures: complete first-party inputs, explicit unknowns.
//!
//! Unknown inputs forbid reuse and coverage. The kind selects the
//! source class: `fmt` binds the independent fmt scope, `init` and
//! `validate` bind the effective load set plus the lockfile. Walks
//! are bounded, skip hidden directories, and treat any symlink as
//! unknown (fail-closed until H5 containment lands in T11).

use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    ClosureBuilder, ContractError, ProposedTask, Provenance, TaskInputClosure,
    canonical_json_bytes, digest_b3, normalize_posix_path,
};

use crate::closure_inputs::{modules_provenance, varfiles_provenance};
use crate::effective::effective_set;
use crate::family::{Family, LOCKFILE_NAME, family_of};
use crate::file_cache::FileCache;
use crate::fmt_scope::is_fmt_file;
use crate::kinds::TofuTaskKind;
use crate::parser::MAX_FILES_PER_UNIT;

/// Resolve one proposed task's closure against the checkout at `root`.
///
/// Reads adapter facts from the proposal (unit path, kind,
/// configuration, drivers, declared inputs); filesystem probes bind
/// content, proven absence, or explicit unknowns.
///
/// # Errors
///
/// Returns [`ContractError`] for task-kind spellings outside the
/// three tofu kinds and for malformed unit paths.
pub fn resolve_closure_at_root(
    root: &Path,
    task: &ProposedTask,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
    reads: &mut FileCache,
) -> Result<TaskInputClosure, ContractError> {
    crate::normalized_root_for_proposal(task)?;
    let kind = TofuTaskKind::parse(&task.task_kind)?;
    let unit = task.identity.unit_path.as_str();
    if !unit.is_empty() {
        normalize_posix_path(unit)?;
    }
    let source_tree = source_tree_provenance(root, unit, kind, &mut *reads);
    let lockfile = lockfile_provenance(root, unit, kind, &mut *reads);
    let modules = modules_provenance(root, unit, kind, &mut *reads);
    let varfiles = varfiles_provenance(root, unit, kind, &mut *reads);
    let mut closure = ClosureBuilder::new()
        .input("source_tree", source_tree)
        .input("lockfile", lockfile)
        .input("modules", modules)
        .input("varfiles", varfiles)
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
        closure = closure.input(&name, probe_path(root, extra, &mut *reads));
    }
    Ok(closure
        .input("vcs", vcs_provenance(task.identity.undeclared_reads))
        .build(&task.task_id))
}

/// Source-tree provenance: fmt scope for `fmt`, effective set otherwise.
fn source_tree_provenance(
    root: &Path,
    unit: &str,
    kind: TofuTaskKind,
    reads: &mut FileCache,
) -> Provenance {
    let collected = match reads.unit_files(root, unit) {
        Ok(collected) => collected,
        Err(reason) => return Provenance::Unknown { reason },
    };
    let wanted: Vec<String> = match kind {
        TofuTaskKind::Fmt => collected
            .iter()
            .filter(|path| is_fmt_file(path.rsplit('/').next().unwrap_or(path)))
            .cloned()
            .collect(),
        TofuTaskKind::InitForValidate | TofuTaskKind::Validate => {
            let configs: Vec<String> = collected
                .into_iter()
                .filter(|path| {
                    matches!(
                        family_of(path.rsplit('/').next().unwrap_or(path)),
                        Family::Config | Family::Override
                    )
                })
                .collect();
            effective_set(&configs)
        }
    };
    if wanted.is_empty() {
        return if kind == TofuTaskKind::Fmt {
            Provenance::AbsentProven {
                evidence: "no_fmt_files".to_owned(),
            }
        } else {
            Provenance::Unknown {
                reason: "no_source_files".to_owned(),
            }
        };
    }
    let mut files = Vec::with_capacity(wanted.len());
    for path in &wanted {
        match reads.read_raw(&root.join(path)) {
            Ok(bytes) => files.push((path.clone(), digest_b3(&bytes))),
            Err(reason) => {
                return Provenance::Unknown {
                    reason: format!("unreadable:{path}:{reason}"),
                };
            }
        }
    }
    files.sort();
    Provenance::Known {
        digest: files_digest(&files),
    }
}

/// Lockfile provenance: content, proven absence, or kind exclusion.
fn lockfile_provenance(
    root: &Path,
    unit: &str,
    kind: TofuTaskKind,
    reads: &mut FileCache,
) -> Provenance {
    if kind == TofuTaskKind::Fmt {
        return Provenance::AbsentProven {
            evidence: "excluded:kind_does_not_read_lockfile".to_owned(),
        };
    }
    let relative = if unit.is_empty() {
        LOCKFILE_NAME.to_owned()
    } else {
        format!("{unit}/{LOCKFILE_NAME}")
    };
    probe_path(root, &relative, reads)
}

/// Provenance of one repo-relative path: digest, absence, or unknown.
fn probe_path(root: &Path, relative: &str, reads: &mut FileCache) -> Provenance {
    let Ok(normalized) = normalize_posix_path(relative) else {
        return Provenance::Unknown {
            reason: format!("bad_path:{relative}"),
        };
    };
    match reads.read_raw(&root.join(&normalized)) {
        Ok(bytes) => Provenance::Known {
            digest: digest_b3(&bytes),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Provenance::AbsentProven {
            evidence: format!("not_found:{normalized}"),
        },
        Err(err) => Provenance::Unknown {
            reason: format!("unreadable:{normalized}:{err}"),
        },
    }
}

/// VCS provenance: unknown with undeclared reads, proven absent without.
fn vcs_provenance(undeclared_reads: bool) -> Provenance {
    if undeclared_reads {
        Provenance::Unknown {
            reason: "tofu_may_observe_vcs".to_owned(),
        }
    } else {
        Provenance::AbsentProven {
            evidence: "offline_tofu_no_vcs_reads".to_owned(),
        }
    }
}

/// Digest over sorted `(path, digest)` file pairs.
pub(crate) fn files_digest(files: &[(String, String)]) -> String {
    let pairs: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, digest)| (path.as_str(), digest.as_str()))
        .collect();
    canonical_json_bytes(&pairs)
        .map_or_else(|_| digest_b3(b"files_error"), |bytes| digest_b3(&bytes))
}

/// Repo-relative files under `unit`, skipping hidden directories.
///
/// Symlinks, non-UTF-8 names, unreadable entries, and over-cap
/// selections fail the walk (the caller reports unknown).
pub(crate) fn collect_unit_files(root: &Path, unit: &str) -> Result<Vec<String>, String> {
    let mut base: PathBuf = root.to_path_buf();
    if !unit.is_empty() {
        base.push(unit);
    }
    let mut files = Vec::new();
    let mut stack = vec![base];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .map_err(|err| format!("unreadable_dir:{}:{err}", current.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|err| format!("unreadable_entry:{}:{err}", current.display()))?;
            if entry
                .file_type()
                .map_err(|err| format!("unreadable_entry:{err}"))?
                .is_symlink()
            {
                return Err(format!("symlink_present:{}", entry.path().display()));
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "non_utf8_name".to_owned())?;
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                if stack.len() >= MAX_FILES_PER_UNIT {
                    return Err("too_many_dirs".to_owned());
                }
                stack.push(path);
                continue;
            }
            if !path.is_file() {
                continue;
            }
            let stripped = path.strip_prefix(root).map_err(|_| "escape".to_owned())?;
            let mut parts = Vec::new();
            for component in stripped.components() {
                parts.push(
                    component
                        .as_os_str()
                        .to_str()
                        .ok_or_else(|| "non_utf8_name".to_owned())?,
                );
            }
            files.push(parts.join("/"));
            if files.len() > MAX_FILES_PER_UNIT {
                return Err(format!("too_many_files:{}", files.len()));
            }
        }
    }
    files.sort();
    Ok(files)
}
