use super::grammar::{ModuleError, ModuleRef, ModuleSource, SourceClass, classify_literal};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleEdge {
    pub from: String,
    pub to: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFinding {
    pub file: String,
    pub name: String,
    pub class: SourceClass,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleEdges {
    pub edges: Vec<ModuleEdge>,
    pub findings: Vec<ModuleFinding>,
}

fn caller_dir(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(dir, _)| dir)
}

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

fn sort_dedupe_edges(edges: &mut Vec<ModuleEdge>) {
    edges.sort_by(|left, right| {
        (&left.from, &left.to, &left.source).cmp(&(&right.from, &right.to, &right.source))
    });
    edges.dedup_by(|curr, prev| {
        curr.from == prev.from && curr.to == prev.to && curr.source == prev.source
    });
}

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
