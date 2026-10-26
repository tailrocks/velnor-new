// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::yaml::{AnchorName, Yaml};
use std::collections::BTreeSet;

pub(super) fn prune(node: &mut Yaml) {
    let mut used = BTreeSet::new();
    collect_uses(node, &mut used);
    strip(node, &used);
}

fn collect_uses(node: &Yaml, used: &mut BTreeSet<AnchorName>) {
    match node {
        Yaml::Alias(name) => {
            used.insert(name.clone());
        }
        Yaml::Map(entries) | Yaml::AnchoredMap { entries, .. } => {
            for (_, value) in entries {
                collect_uses(value, used);
            }
        }
        Yaml::Seq(items) => {
            for value in items {
                collect_uses(value, used);
            }
        }
        _ => {}
    }
}

fn strip(node: &mut Yaml, used: &BTreeSet<AnchorName>) {
    match node {
        Yaml::AnchoredScalar { name, value } if !used.contains(name) => {
            *node = Yaml::Str(std::mem::take(value));
        }
        Yaml::AnchoredMap { name, entries } if !used.contains(name) => {
            let mut entries = std::mem::take(entries);
            for (_, value) in &mut entries {
                strip(value, used);
            }
            *node = Yaml::Map(entries);
        }
        Yaml::Map(entries) | Yaml::AnchoredMap { entries, .. } => {
            for (_, value) in entries {
                strip(value, used);
            }
        }
        Yaml::Seq(items) => {
            for value in items {
                strip(value, used);
            }
        }
        _ => {}
    }
}
