//! Tofu input closures: complete first-party inputs, explicit unknowns.
//!
//! Unknown inputs forbid reuse and coverage. The kind selects the
//! source class: `fmt` binds the independent fmt scope, `init` and
//! `validate` bind the effective load set plus the lockfile. Walks
//! are bounded, skip hidden directories, and treat any symlink as
//! unknown (fail-closed until H5 containment lands in T11).

use std::path::Path;

use velnor_actions_contract::{
    ClosureBuilder, ContractError, Provenance, TaskInputClosure, canonical_json_bytes, digest_b3,
    normalize_posix_path,
};
use velnor_actions_contract_planning::ProposedTask;

use crate::closure_inputs::{modules_provenance, varfiles_provenance};
use velnor_actions_tofu_core::effective::effective_set;
use velnor_actions_tofu_core::family::{Family, LOCKFILE_NAME, family_of};
use velnor_actions_tofu_core::file_cache::FileCache;
use velnor_actions_tofu_core::fmt_scope::is_fmt_file;
use velnor_actions_tofu_core::kinds::TofuTaskKind;

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
