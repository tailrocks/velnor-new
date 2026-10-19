//! Ordered, bounded expansion for the workflow-only sharing projection.

use crate::{
    RenderError,
    yaml::{AnchorName, Yaml, quote_scalar, render_yaml},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 262_144;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
enum Scope {
    Root,
    Jobs,
    Job,
    Steps,
    Step,
    Mapping,
    Run,
    Other,
}

#[derive(Clone, Copy)]
enum AnchorKind {
    Run,
    Mapping,
    Step,
}

pub(super) struct Projection {
    pub(super) value: Yaml,
    pub(super) anchors: BTreeMap<AnchorName, Yaml>,
}

#[derive(Default)]
struct Expansion {
    names: BTreeSet<AnchorName>,
    anchors: BTreeMap<AnchorName, (AnchorKind, Rc<Yaml>)>,
    nodes: usize,
    bytes: usize,
}

pub(super) fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("workflow_sharing:{problem}"))
}

pub(super) fn expand(node: &Yaml) -> Result<Projection, RenderError> {
    let mut expansion = Expansion::default();
    let value = expansion.walk(node, Scope::Root, 0)?;
    if render_yaml(&value).len() > MAX_BYTES {
        return Err(invalid("expanded_bytes"));
    }
    Ok(Projection {
        value,
        anchors: expansion
            .anchors
            .into_iter()
            .map(|(k, (_, v))| (k, (*v).clone()))
            .collect(),
    })
}

impl Expansion {
    fn charge(&mut self, bytes: usize) -> Result<(), RenderError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| invalid("expanded_bytes"))?;
        if self.bytes > MAX_BYTES {
            return Err(invalid("expanded_bytes"));
        }
        Ok(())
    }

    fn walk(&mut self, node: &Yaml, scope: Scope, depth: usize) -> Result<Yaml, RenderError> {
        if depth > MAX_DEPTH {
            return Err(invalid("depth"));
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(invalid("nodes"));
        }
        self.charge(depth * 2 + 8)?;
        match node {
            Yaml::Alias(name) => {
                let (kind, value) = self
                    .anchors
                    .get(name)
                    .cloned()
                    .ok_or_else(|| invalid("unknown_forward_or_cyclic_alias"))?;
                valid_alias(&value, kind, scope)?;
                self.walk(&value, scope, depth + 1)
            }
            Yaml::AnchoredScalar { name, value } => {
                if !matches!(scope, Scope::Run) {
                    return Err(invalid("scalar_anchor_scope"));
                }
                self.name(name)?;
                self.charge(quoted_bytes(value)?)?;
                let value = Yaml::Str(value.clone());
                self.anchors
                    .insert(name.clone(), (AnchorKind::Run, Rc::new(value.clone())));
                Ok(value)
            }
            Yaml::AnchoredMap { name, entries } => {
                if !matches!(scope, Scope::Mapping | Scope::Step) {
                    return Err(invalid("map_anchor_scope"));
                }
                self.name(name)?;
                let value = self.mapping(entries, scope, depth)?;
                let kind = if matches!(scope, Scope::Step) {
                    AnchorKind::Step
                } else {
                    AnchorKind::Mapping
                };
                self.anchors
                    .insert(name.clone(), (kind, Rc::new(value.clone())));
                Ok(value)
            }
            Yaml::Map(entries) => self.mapping(entries, scope, depth),
            Yaml::Seq(items) => {
                let child = if matches!(scope, Scope::Steps) {
                    Scope::Step
                } else {
                    Scope::Other
                };
                Ok(Yaml::Seq(
                    items
                        .iter()
                        .map(|item| self.walk(item, child, depth + 1))
                        .collect::<Result<_, _>>()?,
                ))
            }
            _ => {
                self.scalar(node)?;
                Ok(node.clone())
            }
        }
    }

    fn name(&mut self, name: &AnchorName) -> Result<(), RenderError> {
        if !self.names.insert(name.clone()) {
            return Err(invalid("duplicate_anchor"));
        }
        Ok(())
    }

    fn mapping(
        &mut self,
        entries: &[(String, Yaml)],
        scope: Scope,
        depth: usize,
    ) -> Result<Yaml, RenderError> {
        let mut keys = BTreeSet::new();
        let mut expanded = Vec::new();
        for (key, value) in entries {
            if key == "<<" || !keys.insert(key) {
                return Err(invalid("duplicate_or_merge_key"));
            }
            self.charge(quoted_bytes(key)? + 4)?;
            expanded.push((
                key.clone(),
                self.walk(value, child_scope(scope, key), depth + 1)?,
            ));
        }
        Ok(Yaml::Map(expanded))
    }

    fn scalar(&mut self, node: &Yaml) -> Result<(), RenderError> {
        match node {
            Yaml::Str(value) | Yaml::Quoted(value) => self.charge(quoted_bytes(value)? * 2 + 2),
            Yaml::Annotated { value, comment } => {
                if comment.contains(['\r', '\n']) {
                    return Err(invalid("multiline_comment"));
                }
                self.charge(quoted_bytes(value)? + comment.len() + 4)
            }
            Yaml::Flow(items) => {
                for value in items {
                    self.charge(quoted_bytes(value)? + 4)?;
                }
                Ok(())
            }
            Yaml::FlowMap(entries) => {
                let mut keys = BTreeSet::new();
                for (key, value) in entries {
                    if key == "<<" || !keys.insert(key) {
                        return Err(invalid("duplicate_or_merge_key"));
                    }
                    self.charge(quoted_bytes(key)? + quoted_bytes(value)? + 8)?;
                }
                Ok(())
            }
            Yaml::Null | Yaml::Bool(_) | Yaml::Int(_) => Ok(()),
            _ => Err(invalid("unexpected_scalar")),
        }
    }
}

fn child_scope(scope: Scope, key: &str) -> Scope {
    match (scope, key) {
        (Scope::Root, "jobs") => Scope::Jobs,
        (Scope::Jobs, _) => Scope::Job,
        (Scope::Job, "steps") => Scope::Steps,
        (Scope::Job | Scope::Step, "env") | (Scope::Step, "with") => Scope::Mapping,
        (Scope::Step, "run") => Scope::Run,
        _ => Scope::Other,
    }
}

fn valid_alias(value: &Yaml, kind: AnchorKind, scope: Scope) -> Result<(), RenderError> {
    match (kind, scope, value) {
        (AnchorKind::Run, Scope::Run, Yaml::Str(_))
        | (AnchorKind::Mapping, Scope::Mapping, Yaml::Map(_))
        | (AnchorKind::Step, Scope::Step, Yaml::Map(_)) => Ok(()),
        _ => Err(invalid("alias_scope_or_kind")),
    }
}

fn quoted_bytes(value: &str) -> Result<usize, RenderError> {
    if value.len() > MAX_BYTES {
        return Err(invalid("expanded_bytes"));
    }
    Ok(quote_scalar(value).len())
}
