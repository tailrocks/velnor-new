// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{next_name, validation};
use crate::{
    RenderError,
    yaml::{AnchorName, Yaml, quote_scalar, render_yaml},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Fields,
    Steps,
}

#[derive(Default)]
struct Repeat {
    count: usize,
    total: usize,
    first: usize,
}

pub(super) fn share(
    node: &mut Yaml,
    mode: Mode,
    used: &mut BTreeSet<AnchorName>,
) -> Result<(), RenderError> {
    let anchors = validation::expand(node)?.anchors;
    let mut groups: BTreeMap<String, Repeat> = BTreeMap::new();
    visit(node, mode, &mut |value| {
        if !shareable(value, mode) {
            return Ok(());
        }
        let signature = signature(value, &anchors)?;
        let bytes = render_yaml(value).len();
        let group = groups.entry(signature).or_default();
        if group.count == 0 {
            group.first = bytes;
        }
        group.count += 1;
        group.total = group
            .total
            .checked_add(bytes)
            .ok_or_else(|| validation::invalid("sharing_size"))?;
        Ok(())
    })?;
    let mut named: BTreeMap<String, AnchorName> = BTreeMap::new();
    let mut next = 1;
    visit(node, mode, &mut |value| {
        if !shareable(value, mode) {
            return Ok(());
        }
        let signature = signature(value, &anchors)?;
        if let Some(name) = named.get(&signature) {
            if !has_declaration(value) {
                *value = Yaml::Alias(name.clone());
            }
            return Ok(());
        }
        let group = groups
            .get(&signature)
            .ok_or_else(|| validation::invalid("missing_repeat"))?;
        if group.count < 2 {
            return Ok(());
        }
        let prefix = match mode {
            Mode::Fields => "m",
            Mode::Steps => "s",
        };
        let name = next_name(used, prefix, &mut next);
        let overhead = name.as_str().len() + 4;
        let projected = group
            .first
            .saturating_add(overhead)
            .saturating_add((group.count - 1).saturating_mul(overhead));
        if projected >= group.total {
            return Ok(());
        }
        let Yaml::Map(entries) = value else {
            return Err(validation::invalid("nonmap_candidate"));
        };
        let entries = std::mem::take(entries);
        used.insert(name.clone());
        named.insert(signature, name.clone());
        *value = Yaml::AnchoredMap { name, entries };
        Ok(())
    })
}

fn visit(
    node: &mut Yaml,
    mode: Mode,
    f: &mut impl FnMut(&mut Yaml) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    let Yaml::Map(root) = node else {
        return Err(validation::invalid("root_not_map"));
    };
    for (key, jobs) in root {
        if key != "jobs" {
            continue;
        }
        let Yaml::Map(jobs) = jobs else {
            return Err(validation::invalid("jobs_not_map"));
        };
        for (_, job) in jobs {
            let Yaml::Map(job) = job else {
                return Err(validation::invalid("job_not_map"));
            };
            for (key, value) in job {
                if key == "env" && matches!(mode, Mode::Fields) {
                    candidate(value, mode, f)?;
                } else if key == "steps" {
                    let Yaml::Seq(steps) = value else {
                        return Err(validation::invalid("steps_not_sequence"));
                    };
                    for step in steps {
                        if matches!(mode, Mode::Steps) {
                            candidate(step, mode, f)?;
                            continue;
                        }
                        let Yaml::Map(entries) = step else { continue };
                        for (key, value) in entries {
                            if matches!(key.as_str(), "env" | "with") {
                                candidate(value, mode, f)?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn candidate(
    value: &mut Yaml,
    mode: Mode,
    f: &mut impl FnMut(&mut Yaml) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    match value {
        Yaml::Map(_) => f(value),
        Yaml::FlowMap(_) if matches!(mode, Mode::Fields) => Ok(()),
        Yaml::Alias(_) | Yaml::AnchoredMap { .. } => Ok(()),
        _ => Err(validation::invalid("mapping_candidate_kind")),
    }
}

fn signature(value: &Yaml, anchors: &BTreeMap<AnchorName, Yaml>) -> Result<String, RenderError> {
    Ok(format!("{:?}", normalized(value, anchors)?))
}

fn normalized(value: &Yaml, anchors: &BTreeMap<AnchorName, Yaml>) -> Result<Yaml, RenderError> {
    Ok(match value {
        Yaml::AnchoredScalar { value, .. } => Yaml::Str(value.clone()),
        Yaml::Alias(name) => anchors
            .get(name)
            .cloned()
            .ok_or_else(|| validation::invalid("unknown_signature_alias"))?,
        Yaml::Map(entries) | Yaml::AnchoredMap { entries, .. } => Yaml::Map(
            entries
                .iter()
                .map(|(k, v)| Ok((k.clone(), normalized(v, anchors)?)))
                .collect::<Result<_, RenderError>>()?,
        ),
        Yaml::Seq(items) => Yaml::Seq(
            items
                .iter()
                .map(|v| normalized(v, anchors))
                .collect::<Result<_, _>>()?,
        ),
        other => other.clone(),
    })
}

fn has_declaration(value: &Yaml) -> bool {
    match value {
        Yaml::AnchoredScalar { .. } | Yaml::AnchoredMap { .. } => true,
        Yaml::Map(entries) => entries.iter().any(|(_, v)| has_declaration(v)),
        Yaml::Seq(items) => items.iter().any(has_declaration),
        _ => false,
    }
}

fn shareable(value: &Yaml, mode: Mode) -> bool {
    let Yaml::Map(entries) = value else {
        return false;
    };
    if entries.is_empty() {
        return false;
    }
    match mode {
        Mode::Fields => entries.iter().all(|(key, value)| {
            !key.is_empty()
                && quote_scalar(key) == *key
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                && value.is_inline()
                && !matches!(value, Yaml::Null)
        }),
        Mode::Steps => {
            if entries.first().is_none_or(|(key, value)| {
                key != "name"
                    || !matches!(value,Yaml::Str(name) | Yaml::Quoted(name) if !name.is_empty())
            }) {
                return false;
            }
            if !entries.iter().all(|(key, _)| {
                matches!(
                    key.as_str(),
                    "name"
                        | "id"
                        | "if"
                        | "uses"
                        | "run"
                        | "shell"
                        | "working-directory"
                        | "continue-on-error"
                        | "timeout-minutes"
                        | "env"
                        | "with"
                )
            }) {
                return false;
            }
            render_yaml(value).lines().all(|line| {
                let content = line.trim();
                content.is_empty()
                    || content.starts_with('#')
                    || content.split_once(':').is_some_and(|(key, value)| {
                        !key.is_empty()
                            && !key.bytes().any(|b| b.is_ascii_whitespace())
                            && !value.trim_start().starts_with('!')
                    })
            })
        }
    }
}
