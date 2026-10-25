use std::collections::BTreeSet;

use velnor_actions_contract::{ArchiveInputs, archive_id, parse_strict_json};

use crate::error::MiseError;
use crate::nextest_shapes::NextestArchive;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchivePlan {
    reserved: BTreeSet<String>,
}

impl ArchivePlan {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

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

    #[must_use]
    pub fn len(&self) -> usize {
        self.reserved.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reserved.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortedInventory {
    ids: Vec<String>,
}

impl SortedInventory {
    #[must_use]
    pub fn new(ids: Vec<String>) -> Self {
        let mut sorted = ids;
        sorted.sort();
        sorted.dedup();
        Self { ids: sorted }
    }

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

    #[must_use]
    pub fn ids(&self) -> &[String] {
        &self.ids
    }
}

fn plan_key(package: &str, target: Option<&str>, features: &[String]) -> String {
    format!(
        "{package}\0{}\0{}",
        target.unwrap_or("host"),
        features.join(",")
    )
}

#[derive(Debug, Clone)]
pub struct ArchiveIdentityInputs<'a> {
    pub source_digest: &'a str,
    pub profile: &'a str,
    pub toolchain_id: &'a str,
    pub runtime: &'a str,
    pub test_runner: &'a str,
    pub format: &'a str,
    pub platform_id: &'a str,
    pub config_digest: &'a str,
}

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

#[must_use]
pub fn archive_write_required(shard_count: u32) -> bool {
    shard_count > 1
}

#[must_use]
pub fn requires_archive_transfer(shard_count: u32) -> bool {
    shard_count > 1
}

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
