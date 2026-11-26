// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::context_value::ordinal_ignore_case_eq;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inputs(Vec<(String, String)>);

impl Inputs {
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&String> {
        self.position(name).map(|index| &self.0[index].1)
    }

    #[must_use]
    pub fn contains_key(&self, name: &str) -> bool {
        self.position(name).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.iter().map(|(name, value)| (name, value))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.0
            .iter()
            .position(|(known, _)| ordinal_ignore_case_eq(known, name))
    }
}

impl FromIterator<(String, String)> for Inputs {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(pairs: I) -> Self {
        let mut inputs = Self::default();
        for (name, value) in pairs {
            match inputs.position(&name) {
                Some(index) => inputs.0[index].1 = value,
                None => inputs.0.push((name, value)),
            }
        }
        inputs
    }
}
