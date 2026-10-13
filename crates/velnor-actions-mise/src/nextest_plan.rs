//! Nextest archive planning and inventory ordering (par §8).
//!
//! Archives run once per package/configuration: every archive shares
//! one [`ARCHIVE_FILE`](crate::nextest::ARCHIVE_FILE) path, so a second
//! archive for the same configuration is a typed rejection, whatever
//! driver requested it. Per-partition inventories stay sorted and
//! duplicate-free, enforced at construction.

use std::collections::BTreeSet;

use velnor_actions_contract::{ArchiveInputs, archive_id, parse_strict_json};

use crate::error::MiseError;
use crate::nextest_shapes::NextestArchive;

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

/// Identity inputs for one archive beyond its configuration key.
#[derive(Debug, Clone)]
pub struct ArchiveIdentityInputs<'a> {
    /// Source/input digest.
    pub source_digest: &'a str,
    /// Build profile.
    pub profile: &'a str,
    /// Toolchain identity digest.
    pub toolchain_id: &'a str,
    /// Linker/runtime requirements.
    pub runtime: &'a str,
    /// Exact Nextest version.
    pub test_runner: &'a str,
    /// Archive format.
    pub format: &'a str,
    /// Platform identity digest.
    pub platform_id: &'a str,
    /// Cargo config digest.
    pub config_digest: &'a str,
}

/// Archive identity over the configuration key plus explicit digests.
///
/// Binds package/target/features from `archive` with the real platform
/// and config digests; the orchestrator archives only on this identity.
/// # Errors
pub fn archive_identity(
    archive: &NextestArchive,
    inputs: &ArchiveIdentityInputs<'_>,
) -> Result<String, MiseError> {
    let (package, target, features) = archive.config_key();
    let full = ArchiveInputs {
        source_digest: inputs.source_digest.to_owned(),
        package,
        target: target.unwrap_or_else(|| "host".to_owned()),
        features,
        profile: inputs.profile.to_owned(),
        toolchain_id: inputs.toolchain_id.to_owned(),
        runtime: inputs.runtime.to_owned(),
        test_runner: inputs.test_runner.to_owned(),
        format: inputs.format.to_owned(),
        platform_id: inputs.platform_id.to_owned(),
        config_digest: inputs.config_digest.to_owned(),
    };
    archive_id(&full).map_err(|err| MiseError::Contract {
        problem: err.to_string(),
    })
}

/// Whether `shard_count` needs an archive artifact write.
///
/// Single-shard runs execute from the local build without writing an
/// archive; only sharded runs pay for archive creation.
#[must_use]
pub fn archive_write_required(shard_count: u32) -> bool {
    shard_count > 1
}

/// Whether `shard_count` needs archive artifact transfer.
///
/// Single-shard runs need no transfer; every additional shard consumes
/// the shared archive.
#[must_use]
pub fn requires_archive_transfer(shard_count: u32) -> bool {
    shard_count > 1
}

/// Count test identities in a JSON string-array inventory.
///
/// The planner feeds `nextest list` machine output (validated upstream
/// per PAR-8.19) through the sorted-inventory shape and sizes partitions
/// from this count.
/// # Errors
pub fn count_inventory_tests(json: &str) -> Result<usize, MiseError> {
    let value = parse_strict_json(json).map_err(|err| MiseError::Contract {
        problem: err.to_string(),
    })?;
    let Some(ids) = value.as_array() else {
        return Err(MiseError::InvalidNextestInput {
            field: "inventory".to_owned(),
            value: json.to_owned(),
        });
    };
    for id in ids {
        if id.as_str().is_none() {
            return Err(MiseError::InvalidNextestInput {
                field: "inventory".to_owned(),
                value: json.to_owned(),
            });
        }
    }
    Ok(ids.len())
}
