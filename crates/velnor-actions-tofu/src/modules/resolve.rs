//! Lexical module-edge resolution: references to edges plus findings.
//!
//! Local literals resolve against the caller directory (pure path
//! math; escapes are errors); every other reference becomes a
//! recorded finding. No filesystem access happens here.

use super::grammar::{ModuleError, ModuleRef, ModuleSource, SourceClass, classify_literal};

/// One resolved local edge: caller dir to module dir (`""` = repo root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleEdge {
    /// Caller directory, repo-relative.
    pub from: String,
    /// Module directory, repo-relative.
    pub to: String,
    /// Literal source spelling (identity, never re-resolved).
    pub source: String,
}

/// One non-local reference: recorded, never an edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFinding {
    /// Repo-relative caller file.
    pub file: String,
    /// Module block label.
    pub name: String,
    /// Finding class (never [`SourceClass::Local`]).
    pub class: SourceClass,
    /// Source literal, or the dynamic reason.
    pub detail: String,
}

/// Resolved local edges plus recorded findings (selection inputs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleEdges {
    /// Sorted deduplicated local edges.
    pub edges: Vec<ModuleEdge>,
    /// Sorted deduplicated findings.
    pub findings: Vec<ModuleFinding>,
}

/// Parent directory of a repo-relative path; empty for the root.
fn caller_dir(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Lexically resolve a local `source` against `caller_dir` (`""` = root).
///
/// Dot segments collapse; `None` when the target escapes the repository.
#[must_use]
pub fn resolve_local_target(caller_dir: &str, source: &str) -> Option<String> {
    let mut stack: Vec<&str> = caller_dir
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    for segment in source.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            _ => stack.push(segment),
        }
    }
    Some(stack.join("/"))
}

/// Resolve references to local edges plus findings (lexical only).
///
/// # Errors
///
/// Returns [`ModuleError::Escape`] for a local source escaping the repo.
pub fn resolve_refs(refs: &[ModuleRef]) -> Result<ModuleEdges, ModuleError> {
    let mut edges = Vec::new();
    let mut findings = Vec::new();
    for reference in refs {
        let caller = caller_dir(&reference.file);
        match &reference.source {
            ModuleSource::Literal(source) => match classify_literal(source) {
                SourceClass::Local => {
                    let Some(target) = resolve_local_target(caller, source) else {
                        return Err(ModuleError::Escape {
                            target: format!("{caller}:{source}"),
                        });
                    };
                    edges.push(ModuleEdge {
                        from: caller.to_owned(),
                        to: target,
                        source: source.clone(),
                    });
                }
                class => findings.push(ModuleFinding {
                    file: reference.file.clone(),
                    name: reference.name.clone(),
                    class,
                    detail: source.clone(),
                }),
            },
            ModuleSource::Dynamic { reason } => findings.push(ModuleFinding {
                file: reference.file.clone(),
                name: reference.name.clone(),
                class: SourceClass::Dynamic,
                detail: reason.clone(),
            }),
        }
    }
    sort_dedupe_edges(&mut edges);
    sort_dedupe_findings(&mut findings);
    Ok(ModuleEdges { edges, findings })
}

/// Sort edges by (`from`, `to`, `source`) and drop duplicates.
fn sort_dedupe_edges(edges: &mut Vec<ModuleEdge>) {
    edges.sort_by(|left, right| {
        (&left.from, &left.to, &left.source).cmp(&(&right.from, &right.to, &right.source))
    });
    edges.dedup_by(|curr, prev| {
        curr.from == prev.from && curr.to == prev.to && curr.source == prev.source
    });
}

/// Sort findings by identity and drop duplicates.
fn sort_dedupe_findings(findings: &mut Vec<ModuleFinding>) {
    findings.sort_by(|left, right| {
        (
            &left.file,
            &left.name,
            &left.detail,
            format!("{:?}", left.class),
        )
            .cmp(&(
                &right.file,
                &right.name,
                &right.detail,
                format!("{:?}", right.class),
            ))
    });
    findings.dedup_by(|curr, prev| {
        curr.file == prev.file
            && curr.name == prev.name
            && curr.detail == prev.detail
            && curr.class == prev.class
    });
}
