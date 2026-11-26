// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use std::fmt;
use std::marker::PhantomData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedContextData<T> {
    entries: Vec<(String, T)>,
}

impl<T> Default for OrderedContextData<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<T> OrderedContextData<T> {
    pub fn insert(&mut self, key: String, value: T) -> Option<T> {
        if let Some((_, existing)) = self.entries.iter_mut().find(|(name, _)| name == &key) {
            Some(std::mem::replace(existing, value))
        } else {
            self.entries.push((key, value));
            None
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, (String, T)> {
        self.entries.iter()
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&T> {
        self.entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'de, T> Deserialize<'de> for OrderedContextData<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OrderedContextDataVisitor<T>(PhantomData<T>);

        impl<'de, T> Visitor<'de> for OrderedContextDataVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = OrderedContextData<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an ordered map")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = OrderedContextData::default();
                while let Some((key, value)) = map.next_entry()? {
                    entries.insert(key, value);
                }
                Ok(entries)
            }
        }

        deserializer.deserialize_map(OrderedContextDataVisitor(PhantomData))
    }
}

impl<'a, T> IntoIterator for &'a OrderedContextData<T> {
    type Item = &'a (String, T);
    type IntoIter = std::slice::Iter<'a, (String, T)>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
