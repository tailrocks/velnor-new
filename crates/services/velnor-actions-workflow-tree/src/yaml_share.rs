//! Share repeated workflow `run` scalars when YAML aliases reduce output size.

use std::collections::BTreeMap;

use crate::yaml::{AnchorName, Yaml, quote_scalar};

pub(super) fn share_repeated_run_scalars(node: Yaml) -> Yaml {
    let mut counts = BTreeMap::new();
    count_run_scalars(&node, &mut counts);
    share_run_scalars(node, &counts, &mut BTreeMap::new(), &mut 1)
}

fn count_run_scalars(node: &Yaml, counts: &mut BTreeMap<String, usize>) {
    match node {
        Yaml::Map(entries) | Yaml::AnchoredMap { entries, .. } => {
            for (key, value) in entries {
                if key == "run" {
                    if let Yaml::Str(command) = value {
                        *counts.entry(command.clone()).or_default() += 1;
                    }
                } else {
                    count_run_scalars(value, counts);
                }
            }
        }
        Yaml::Seq(items) => {
            for item in items {
                count_run_scalars(item, counts);
            }
        }
        Yaml::Null
        | Yaml::Str(_)
        | Yaml::Bool(_)
        | Yaml::Int(_)
        | Yaml::Flow(_)
        | Yaml::Quoted(_)
        | Yaml::Annotated { .. }
        | Yaml::AnchoredScalar { .. }
        | Yaml::Alias(_) => {}
    }
}

fn share_run_scalars(
    node: Yaml,
    counts: &BTreeMap<String, usize>,
    anchors: &mut BTreeMap<String, AnchorName>,
    next_anchor: &mut usize,
) -> Yaml {
    match node {
        Yaml::Map(entries) => Yaml::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    if key == "run" {
                        (key, share_run_value(value, counts, anchors, next_anchor))
                    } else {
                        (key, share_run_scalars(value, counts, anchors, next_anchor))
                    }
                })
                .collect(),
        ),
        Yaml::AnchoredMap { name, entries } => Yaml::AnchoredMap {
            name,
            entries: entries
                .into_iter()
                .map(|(key, value)| {
                    if key == "run" {
                        (key, share_run_value(value, counts, anchors, next_anchor))
                    } else {
                        (key, share_run_scalars(value, counts, anchors, next_anchor))
                    }
                })
                .collect(),
        },
        Yaml::Seq(items) => Yaml::Seq(
            items
                .into_iter()
                .map(|item| share_run_scalars(item, counts, anchors, next_anchor))
                .collect(),
        ),
        other => other,
    }
}

fn share_run_value(
    value: Yaml,
    counts: &BTreeMap<String, usize>,
    anchors: &mut BTreeMap<String, AnchorName>,
    next_anchor: &mut usize,
) -> Yaml {
    let Yaml::Str(command) = value else {
        return value;
    };
    if counts.get(&command).copied().unwrap_or_default() < 2 {
        return Yaml::Str(command);
    }
    let name = anchors
        .get(&command)
        .cloned()
        .or_else(|| AnchorName::new(format!("velnor_run_{}", *next_anchor)));
    let Some(name) = name else {
        return Yaml::Str(command);
    };
    if !run_anchor_saves_bytes(&command, counts[&command], &name) {
        return Yaml::Str(command);
    }
    if let Some(existing) = anchors.get(&command) {
        return Yaml::Alias(existing.clone());
    }
    *next_anchor = next_anchor.saturating_add(1);
    anchors.insert(command.clone(), name.clone());
    Yaml::AnchoredScalar {
        name,
        value: command,
    }
}

fn run_anchor_saves_bytes(command: &str, occurrences: usize, name: &AnchorName) -> bool {
    let scalar_bytes = quote_scalar(command).len();
    let anchor_bytes = name.as_str().len() + 2;
    let alias_bytes = name.as_str().len() + 1;
    scalar_bytes.saturating_mul(occurrences)
        > scalar_bytes
            .saturating_add(anchor_bytes)
            .saturating_add(alias_bytes.saturating_mul(occurrences.saturating_sub(1)))
}
