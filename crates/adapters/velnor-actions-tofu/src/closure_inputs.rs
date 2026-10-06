//! Tofu closure inputs for modules (M2) and auto varfiles (H2).
//!
//! The `modules` input binds local-module source spellings to
//! resolved canonical targets and their effective content digests;
//! findings, escapes, missing targets, and cycles resolve to
//! Unknown with a typed reason. The `varfiles` input binds
//! committed auto-loaded varfiles per unit. `fmt` excludes both
//! (it resolves no modules and binds tfvars via `source_tree`).

use std::path::Path;

use velnor_actions_contract::{Provenance, digest_b3};

use crate::closure::files_digest;
use velnor_actions_tofu_core::effective::effective_set;
use velnor_actions_tofu_core::family::{Family, family_of, is_auto_var};
use velnor_actions_tofu_core::file_cache::FileCache;
use velnor_actions_tofu_core::kinds::TofuTaskKind;
use velnor_actions_tofu_core::modules::{
    ModuleEdge, ModuleError, ModuleFinding, ModuleRef, ModuleSource, SourceClass,
    canonicalize_side, check_acyclic, identities_digest, resolve_local_target, resolve_refs,
};

/// One module identity record: `(file, name, source, target, content)`.
type IdentityRecord = (String, String, String, String, String);

/// Module source/content identity (M2), or Unknown with a typed reason.
pub(crate) fn modules_provenance(
    root: &Path,
    unit: &str,
    kind: TofuTaskKind,
    reads: &mut FileCache,
) -> Provenance {
    if kind == TofuTaskKind::Fmt {
        return Provenance::AbsentProven {
            evidence: "excluded:kind_does_not_resolve_modules".to_owned(),
        };
    }
    let collected = match reads.unit_files(root, unit) {
        Ok(collected) => collected,
        Err(reason) => return Provenance::Unknown { reason },
    };
    let configs: Vec<String> = collected
        .into_iter()
        .filter(|path| {
            matches!(
                family_of(path.rsplit('/').next().unwrap_or(path)),
                Family::Config | Family::Override
            )
        })
        .collect();
    let wanted = effective_set(&configs);
    let refs = match unit_module_refs(root, &wanted, &mut *reads) {
        Ok(refs) => refs,
        Err(reason) => return Provenance::Unknown { reason },
    };
    let resolved = match resolve_refs(&refs) {
        Ok(resolved) => resolved,
        Err(err) => {
            return Provenance::Unknown {
                reason: err.to_string(),
            };
        }
    };
    if let Some(first) = resolved.findings.first() {
        return Provenance::Unknown {
            reason: finding_reason(first),
        };
    }
    let Ok(canonical_root) = root.canonicalize() else {
        return Provenance::Unknown {
            reason: ModuleError::Unreadable {
                target: String::new(),
            }
            .to_string(),
        };
    };
    let edges = match canonical_edges(&canonical_root, &resolved.edges) {
        Ok(edges) => edges,
        Err(err) => {
            return Provenance::Unknown {
                reason: err.to_string(),
            };
        }
    };
    if let Err(err) = check_acyclic(&edges) {
        return Provenance::Unknown {
            reason: err.to_string(),
        };
    }
    let records = match identity_records(&canonical_root, &refs, reads) {
        Ok(records) => records,
        Err(err) => {
            return Provenance::Unknown {
                reason: err.to_string(),
            };
        }
    };
    let borrowed: Vec<(&str, &str, &str, &str, &str)> = records
        .iter()
        .map(|(file, name, source, target, content)| {
            (
                file.as_str(),
                name.as_str(),
                source.as_str(),
                target.as_str(),
                content.as_str(),
            )
        })
        .collect();
    Provenance::Known {
        digest: identities_digest(&borrowed),
    }
}

/// Identity records for every reference, in reference order.
fn identity_records(
    canonical_root: &Path,
    refs: &[ModuleRef],
    reads: &mut FileCache,
) -> Result<Vec<IdentityRecord>, ModuleError> {
    let mut records = Vec::with_capacity(refs.len());
    for reference in refs {
        records.push(identity_record(canonical_root, reference, &mut *reads)?);
    }
    Ok(records)
}

/// H2 auto-loaded varfiles identity, or proven absence.
pub(crate) fn varfiles_provenance(
    root: &Path,
    unit: &str,
    kind: TofuTaskKind,
    reads: &mut FileCache,
) -> Provenance {
    if kind == TofuTaskKind::Fmt {
        return Provenance::AbsentProven {
            evidence: "excluded:fmt_binds_tfvars_via_source_tree".to_owned(),
        };
    }
    let collected = match reads.unit_files(root, unit) {
        Ok(collected) => collected,
        Err(reason) => return Provenance::Unknown { reason },
    };
    let mut wanted: Vec<String> = collected
        .into_iter()
        .filter(|path| is_auto_var(path.rsplit('/').next().unwrap_or(path)))
        .collect();
    if wanted.is_empty() {
        return Provenance::AbsentProven {
            evidence: "no_auto_varfiles".to_owned(),
        };
    }
    wanted.sort();
    let mut files = Vec::with_capacity(wanted.len());
    for path in &wanted {
        match reads.read_raw(&root.join(path)) {
            Ok(bytes) => files.push((path.clone(), digest_b3(&bytes))),
            Err(err) => {
                return Provenance::Unknown {
                    reason: format!("unreadable:{path}:{err}"),
                };
            }
        }
    }
    files.sort();
    Provenance::Known {
        digest: files_digest(&files),
    }
}

/// Parse module references from the unit's effective configs.
fn unit_module_refs(
    root: &Path,
    wanted: &[String],
    reads: &mut FileCache,
) -> Result<Vec<ModuleRef>, String> {
    let mut refs = Vec::new();
    for path in wanted {
        let model = match reads.model_for(root, path) {
            Ok(Some(model)) => model,
            Ok(None) => continue,
            Err(reason) => return Err(reason),
        };
        for decl in &model.modules {
            refs.push(ModuleRef {
                file: path.clone(),
                name: decl.name.clone(),
                source: decl.source.clone(),
            });
        }
    }
    Ok(refs)
}

/// Canonicalize resolved edges against the canonical checkout root.
fn canonical_edges(
    canonical_root: &Path,
    edges: &[ModuleEdge],
) -> Result<Vec<ModuleEdge>, ModuleError> {
    let mut canonical = Vec::with_capacity(edges.len());
    for edge in edges {
        canonical.push(ModuleEdge {
            from: canonicalize_side(canonical_root, &edge.from)?,
            to: canonicalize_side(canonical_root, &edge.to)?,
            source: edge.source.clone(),
        });
    }
    Ok(canonical)
}

/// Unknown reason naming the first sorted finding (identity recorded).
fn finding_reason(finding: &ModuleFinding) -> String {
    let class = match finding.class {
        SourceClass::Dynamic => "dynamic_source",
        SourceClass::External => "external_source",
        SourceClass::Remote(_) => "remote_source",
        SourceClass::Local => "local_source",
    };
    format!(
        "{class}:{}:{}:{}",
        finding.file, finding.name, finding.detail
    )
}

/// One identity record: `(file, name, source, canonical target, content)`.
///
/// Findings returned Unknown before this point, so only local
/// literals arrive; anything else errors (unreachable in practice).
fn identity_record(
    canonical_root: &Path,
    reference: &ModuleRef,
    reads: &mut FileCache,
) -> Result<IdentityRecord, ModuleError> {
    let ModuleSource::Literal(source) = &reference.source else {
        return Err(ModuleError::Unreadable {
            target: reference.file.clone(),
        });
    };
    let caller = reference.file.rsplit_once('/').map_or("", |(dir, _)| dir);
    let Some(lexical) = resolve_local_target(caller, source) else {
        return Err(ModuleError::Escape {
            target: format!("{caller}:{source}"),
        });
    };
    let canonical = canonicalize_side(canonical_root, &lexical)?;
    let content = digest_target(canonical_root, &canonical, reads)?;
    Ok((
        reference.file.clone(),
        reference.name.clone(),
        source.clone(),
        canonical,
        content,
    ))
}

/// Digest the effective config content of one canonical target dir.
fn digest_target(
    canonical_root: &Path,
    target: &str,
    reads: &mut FileCache,
) -> Result<String, ModuleError> {
    let walked = reads
        .unit_files(canonical_root, target)
        .map_err(|reason| walk_error(target, &reason))?;
    let configs: Vec<String> = walked
        .into_iter()
        .filter(|path| {
            matches!(
                family_of(path.rsplit('/').next().unwrap_or(path)),
                Family::Config | Family::Override
            )
        })
        .collect();
    let wanted = effective_set(&configs);
    if wanted.is_empty() {
        return Err(ModuleError::MissingTarget {
            target: target.to_owned(),
        });
    }
    let mut files = Vec::with_capacity(wanted.len());
    for path in &wanted {
        match reads.read_raw(&canonical_root.join(path)) {
            Ok(bytes) => files.push((path.clone(), digest_b3(&bytes))),
            Err(_) => {
                return Err(ModuleError::Unreadable {
                    target: target.to_owned(),
                });
            }
        }
    }
    files.sort();
    Ok(files_digest(&files))
}

/// Map a target-walk failure to a typed module error.
fn walk_error(target: &str, reason: &str) -> ModuleError {
    if let Some(count) = reason
        .strip_prefix("too_many_files:")
        .and_then(|digits| digits.parse::<usize>().ok())
    {
        ModuleError::TooManyFiles { count }
    } else {
        ModuleError::Unreadable {
            target: target.to_owned(),
        }
    }
}
