use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use crate::{
    RenderError,
    yaml::{AnchorName, Yaml, quote_scalar, render_yaml},
};

const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 262_144;
const MAX_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn materialize(value: &Yaml) -> Result<Yaml, RenderError> {
    let mut expansion = Expansion::default();
    let value = expansion.walk(value, 0)?;
    if render_yaml(&value).len() > MAX_BYTES {
        return Err(invalid("expanded_bytes"));
    }
    Ok(value)
}

struct Expansion {
    anchors: BTreeMap<AnchorName, Rc<Yaml>>,
    names: BTreeSet<AnchorName>,
    nodes: usize,
    bytes: usize,
    max_depth: usize,
    max_nodes: usize,
    max_bytes: usize,
}

impl Default for Expansion {
    fn default() -> Self {
        Self {
            anchors: BTreeMap::new(),
            names: BTreeSet::new(),
            nodes: 0,
            bytes: 0,
            max_depth: MAX_DEPTH,
            max_nodes: MAX_NODES,
            max_bytes: MAX_BYTES,
        }
    }
}

impl Expansion {
    fn walk(&mut self, value: &Yaml, depth: usize) -> Result<Yaml, RenderError> {
        self.nodes = self.nodes.saturating_add(1);
        if depth > self.max_depth || self.nodes > self.max_nodes {
            return Err(invalid("expanded_limit"));
        }
        match value {
            Yaml::Alias(name) => {
                let value = Rc::clone(
                    self.anchors
                        .get(name)
                        .ok_or_else(|| invalid("unknown_alias"))?,
                );
                self.walk(&value, depth + 1)
            }
            Yaml::AnchoredScalar { name, value } => {
                self.register(name)?;
                self.charge(quote_scalar(value).len())?;
                let expanded = Yaml::Str(value.clone());
                self.anchors.insert(name.clone(), Rc::new(expanded.clone()));
                Ok(expanded)
            }
            Yaml::AnchoredMap { name, entries } => {
                self.register(name)?;
                let expanded = self.map(entries, depth)?;
                self.anchors.insert(name.clone(), Rc::new(expanded.clone()));
                Ok(expanded)
            }
            Yaml::Map(entries) => self.map(entries, depth),
            Yaml::Seq(items) => Ok(Yaml::Seq(
                items
                    .iter()
                    .map(|item| self.walk(item, depth + 1))
                    .collect::<Result<_, _>>()?,
            )),
            _ => {
                self.charge(render_yaml(value).len())?;
                Ok(value.clone())
            }
        }
    }

    fn map(&mut self, entries: &[(String, Yaml)], depth: usize) -> Result<Yaml, RenderError> {
        let mut output = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            self.charge(key.len().saturating_add(4))?;
            output.push((key.clone(), self.walk(value, depth + 1)?));
        }
        Ok(Yaml::Map(output))
    }

    fn register(&mut self, name: &AnchorName) -> Result<(), RenderError> {
        if !self.names.insert(name.clone()) {
            return Err(invalid("duplicate_anchor"));
        }
        Ok(())
    }

    fn charge(&mut self, bytes: usize) -> Result<(), RenderError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| invalid("expanded_bytes"))?;
        if self.bytes > self.max_bytes {
            return Err(invalid("expanded_bytes"));
        }
        Ok(())
    }
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("workflow_routing_alias:{problem}"))
}

#[cfg(test)]
#[path = "workflow_routing_aliases_tests.rs"]
mod tests;
