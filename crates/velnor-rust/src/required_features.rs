// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::RequiredFeature;

fn reaches(features: &BTreeMap<String, Vec<String>>, start: &str, name: &str) -> bool {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut pending = vec![start];
    while let Some(feature) = pending.pop() {
        if !seen.insert(feature) {
            continue;
        }
        for implied in features.get(feature).into_iter().flatten() {
            if implied == name {
                return true;
            }
            pending.push(implied);
        }
    }
    false
}

pub(crate) fn resolve(
    features: &BTreeMap<String, Vec<String>>,
    names: &[String],
) -> Vec<RequiredFeature> {
    names
        .iter()
        .map(|name| RequiredFeature {
            name: name.clone(),
            enabled_by: features
                .keys()
                .filter(|feature| *feature != name && reaches(features, feature, name))
                .cloned()
                .collect(),
        })
        .collect()
}
