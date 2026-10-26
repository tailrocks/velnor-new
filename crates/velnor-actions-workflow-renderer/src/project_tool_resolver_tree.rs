// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_contract::config::project_tool_refs::{
    project_tool_ref_names, project_tool_refs_to_expressions,
};

use crate::{RenderError, yaml::Yaml};

use super::quote_run_refs;

pub(super) fn rewrite_mappings(node: &mut Yaml, in_run: bool) -> Result<(), RenderError> {
    let rewrite = |text: &mut String| -> Result<(), RenderError> {
        *text = if in_run {
            quote_run_refs(text)?
        } else {
            project_tool_refs_to_expressions(text)
        };
        Ok(())
    };
    match node {
        Yaml::Str(text) | Yaml::Quoted(text) => rewrite(text)?,
        Yaml::Annotated { value, .. } | Yaml::AnchoredScalar { value, .. } => rewrite(value)?,
        Yaml::Flow(items) => {
            for item in items {
                rewrite(item)?;
            }
        }
        Yaml::FlowMap(pairs) => {
            for (_, value) in pairs {
                rewrite(value)?;
            }
        }
        Yaml::Seq(items) => {
            for item in items {
                rewrite_mappings(item, in_run)?;
            }
        }
        Yaml::Map(pairs) | Yaml::AnchoredMap { entries: pairs, .. } => {
            for (key, value) in pairs {
                rewrite_mappings(value, key == "run")?;
            }
        }
        Yaml::Null | Yaml::Bool(_) | Yaml::Int(_) | Yaml::Alias(_) => {}
    }
    Ok(())
}

pub(super) fn names(node: &Yaml) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect(node, &mut out);
    out
}

fn add(out: &mut BTreeSet<String>, text: &str) {
    out.extend(project_tool_ref_names(text).into_iter().map(str::to_owned));
}

fn collect(node: &Yaml, out: &mut BTreeSet<String>) {
    match node {
        Yaml::Str(text) | Yaml::Quoted(text) => add(out, text),
        Yaml::Annotated { value, .. } | Yaml::AnchoredScalar { value, .. } => add(out, value),
        Yaml::Flow(items) => {
            for item in items {
                add(out, item);
            }
        }
        Yaml::FlowMap(pairs) => {
            for (_, value) in pairs {
                add(out, value);
            }
        }
        Yaml::Seq(items) => {
            for item in items {
                collect(item, out);
            }
        }
        Yaml::Map(pairs) | Yaml::AnchoredMap { entries: pairs, .. } => {
            for (_, value) in pairs {
                collect(value, out);
            }
        }
        Yaml::Null | Yaml::Bool(_) | Yaml::Int(_) | Yaml::Alias(_) => {}
    }
}

pub(super) fn scalar(node: &Yaml) -> Option<&str> {
    match node {
        Yaml::Str(text) | Yaml::Quoted(text) => Some(text),
        Yaml::Annotated { value, .. } | Yaml::AnchoredScalar { value, .. } => Some(value),
        _ => None,
    }
}
