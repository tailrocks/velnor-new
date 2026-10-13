//! Minimal, allocation-bounded Docker inventory response types.

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::de::{Deserializer, Error as DeError, IgnoredAny, MapAccess, SeqAccess, Visitor};

use super::{MAX_CONTAINER_NAMES, MAX_INVENTORY_OBJECTS_PER_KIND, MAX_LABEL_COUNT};

const MAX_VOLUME_WARNINGS: usize = 64;

/// A JSON array that rejects the first item past its limit before growing its vector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BoundedVec<T, const MAX: usize>(Vec<T>);

impl<T, const MAX: usize> BoundedVec<T, MAX> {
    pub(super) fn into_vec(self) -> Vec<T> {
        self.0
    }
}

impl<'de, T, const MAX: usize> Deserialize<'de> for BoundedVec<T, MAX>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BoundedVisitor<T, const MAX: usize>(PhantomData<T>);

        impl<'de, T, const MAX: usize> Visitor<'de> for BoundedVisitor<T, MAX>
        where
            T: Deserialize<'de>,
        {
            type Value = BoundedVec<T, MAX>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "an array with at most {MAX} items")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let initial_capacity = sequence.size_hint().unwrap_or(0).min(256).min(MAX);
                let mut values = Vec::with_capacity(initial_capacity);
                loop {
                    if values.len() == MAX {
                        if sequence.next_element::<IgnoredAny>()?.is_some() {
                            return Err(A::Error::custom("inventory array exceeds item limit"));
                        }
                        break;
                    }
                    match sequence.next_element()? {
                        Some(value) => values.push(value),
                        None => break,
                    }
                }
                Ok(BoundedVec(values))
            }
        }

        deserializer.deserialize_seq(BoundedVisitor::<T, MAX>(PhantomData))
    }
}

/// A JSON string map that bounds bucket allocation and rejects duplicate keys.
#[derive(Debug, Clone)]
pub(super) struct BoundedMap<K, V, const MAX: usize>(HashMap<K, V>);

impl<K, V, const MAX: usize> Default for BoundedMap<K, V, MAX> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<K, V, const MAX: usize> BoundedMap<K, V, MAX> {
    pub(super) fn into_map(self) -> HashMap<K, V> {
        self.0
    }
}

impl<'de, K, V, const MAX: usize> Deserialize<'de> for BoundedMap<K, V, MAX>
where
    K: Deserialize<'de> + Eq + Hash,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BoundedMapVisitor<K, V, const MAX: usize>(PhantomData<(K, V)>);

        impl<'de, K, V, const MAX: usize> Visitor<'de> for BoundedMapVisitor<K, V, MAX>
        where
            K: Deserialize<'de> + Eq + Hash,
            V: Deserialize<'de>,
        {
            type Value = BoundedMap<K, V, MAX>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "a map with at most {MAX} unique keys")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let initial_capacity = map.size_hint().unwrap_or(0).min(32).min(MAX);
                let mut values = HashMap::with_capacity(initial_capacity);
                loop {
                    if values.len() == MAX {
                        if map.next_key::<IgnoredAny>()?.is_some() {
                            return Err(A::Error::custom("inventory map exceeds key limit"));
                        }
                        break;
                    }
                    let Some(key) = map.next_key()? else {
                        break;
                    };
                    if values.contains_key(&key) {
                        return Err(A::Error::custom("inventory map contains duplicate key"));
                    }
                    let value = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(BoundedMap(values))
            }
        }

        deserializer.deserialize_map(BoundedMapVisitor::<K, V, MAX>(PhantomData))
    }
}

/// Only fields used by host ownership reconciliation are materialized.
/// Unknown Docker API fields are parsed as ignored values, not allocated into
/// Bollard's full nested model.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct InventoryContainer {
    #[serde(rename = "Id")]
    pub(super) id: Option<String>,
    #[serde(rename = "Names")]
    pub(super) names: Option<BoundedVec<String, MAX_CONTAINER_NAMES>>,
    #[serde(rename = "Labels")]
    pub(super) labels: Option<BoundedMap<String, String, MAX_LABEL_COUNT>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct InventoryNetwork {
    #[serde(rename = "Id")]
    pub(super) id: Option<String>,
    #[serde(rename = "Name")]
    pub(super) name: Option<String>,
    #[serde(rename = "Labels")]
    pub(super) labels: Option<BoundedMap<String, String, MAX_LABEL_COUNT>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct InventoryVolume {
    #[serde(rename = "Name")]
    pub(super) name: Option<String>,
    #[serde(rename = "Labels")]
    pub(super) labels: Option<BoundedMap<String, String, MAX_LABEL_COUNT>>,
}

/// Volume-list envelope with bounded volume and warning arrays.
#[derive(Debug, Deserialize)]
pub(super) struct BoundedVolumeListResponse {
    #[serde(rename = "Volumes")]
    volumes: Option<BoundedVec<InventoryVolume, MAX_INVENTORY_OBJECTS_PER_KIND>>,
    #[serde(rename = "Warnings")]
    warnings: Option<BoundedVec<String, MAX_VOLUME_WARNINGS>>,
}

impl BoundedVolumeListResponse {
    pub(super) fn into_volumes(self) -> Result<Vec<InventoryVolume>, crate::HostError> {
        if self
            .warnings
            .is_some_and(|warnings| !warnings.into_vec().is_empty())
        {
            return Err(crate::HostError::Docker);
        }
        self.volumes
            .map(BoundedVec::into_vec)
            .ok_or(crate::HostError::Docker)
    }
}

#[cfg(test)]
#[path = "response/tests.rs"]
mod tests;
