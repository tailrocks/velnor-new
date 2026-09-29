//! Nextest archive planning and inventory ordering (par §8).
//!
//! Archives run once per package/configuration: every archive shares
//! one [`ARCHIVE_FILE`](crate::nextest::ARCHIVE_FILE) path, so a second
//! archive for the same configuration is a typed rejection, whatever
//! driver requested it. Per-partition inventories stay sorted and
//! duplicate-free, enforced at construction.

use std::collections::BTreeSet;

use crate::error::MiseError;
use crate::nextest::NextestArchive;

/// Archive-once plan: one configuration key per planned archive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchivePlan {
    /// Reserved `package \0 target \0 features` keys.
    reserved: BTreeSet<String>,
}

impl ArchivePlan {
    /// Empty plan reserving no configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve one archive; a repeated configuration fails.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidNextestInput`] when the archive's
    /// package, target, and features repeat a reserved configuration.
    pub fn add(&mut self, archive: &NextestArchive) -> Result<(), MiseError> {
        let (package, target, features) = archive.config_key();
        let key = plan_key(&package, target.as_deref(), &features);
        if self.reserved.insert(key.clone()) {
            Ok(())
        } else {
            Err(MiseError::InvalidNextestInput {
                field: "archive_plan".to_owned(),
                value: key,
            })
        }
    }

    /// Count of reserved configurations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.reserved.len()
    }

    /// Whether no configuration is reserved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reserved.is_empty()
    }
}

/// Per-partition sorted test-identity inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortedInventory {
    /// Sorted duplicate-free test identities.
    ids: Vec<String>,
}

impl SortedInventory {
    /// Build an inventory, sorting and deduplicating inside.
    #[must_use]
    pub fn new(ids: Vec<String>) -> Self {
        let mut sorted = ids;
        sorted.sort();
        sorted.dedup();
        Self { ids: sorted }
    }

    /// Accept an inventory only when already sorted and duplicate-free.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidNextestInput`] for an unsorted or
    /// duplicated identity list.
    pub fn from_sorted(ids: Vec<String>) -> Result<Self, MiseError> {
        let mut ordered = ids.clone();
        ordered.sort();
        ordered.dedup();
        if ordered == ids {
            Ok(Self { ids })
        } else {
            Err(MiseError::InvalidNextestInput {
                field: "inventory".to_owned(),
                value: ids.join(","),
            })
        }
    }

    /// Sorted duplicate-free test identities.
    #[must_use]
    pub fn ids(&self) -> &[String] {
        &self.ids
    }
}

/// Encode one configuration key; NUL separators block ambiguity.
fn plan_key(package: &str, target: Option<&str>, features: &[String]) -> String {
    format!(
        "{package}\0{}\0{}",
        target.unwrap_or("host"),
        features.join(",")
    )
}
