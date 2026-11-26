// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs(BTreeMap<String, (String, String)>);

impl Inputs {
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&String> {
        self.0.get(&name.to_lowercase()).map(|(_, value)| value)
    }

    #[must_use]
    pub fn contains_key(&self, name: &str) -> bool {
        self.0.contains_key(&name.to_lowercase())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.values().map(|(name, value)| (name, value))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<(String, String)> for Inputs {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(pairs: I) -> Self {
        let mut inputs = BTreeMap::new();
        for (name, value) in pairs {
            inputs
                .entry(name.to_lowercase())
                .and_modify(|(_, known): &mut (String, String)| known.clone_from(&value))
                .or_insert((name, value));
        }
        Self(inputs)
    }
}
