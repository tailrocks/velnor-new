// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;
use crate::workflow::cache_identity::Gc1Family;

use super::{
    CacheFamily, FamilySpec, GC1_HARD_CEILING_BYTES, GC1_MAX_GENERATION_SLOTS, ROOT_PLACEHOLDER,
    RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL,
};

/// Common contract policy shared by every cache family.
///
/// Each family keeps its own payload distinction. No family carries an
/// independent quota; admission always applies the one decimal ceiling
/// with the common generation-slot limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilyContractPolicy {
    pub gc1: Option<Gc1Family>,
    pub hard_ceiling_bytes: u64,
    pub max_generation_slots: u64,
}

#[must_use]
pub fn family_contract_policy(family: CacheFamily) -> FamilyContractPolicy {
    FamilyContractPolicy {
        gc1: family.gc1_family(),
        hard_ceiling_bytes: GC1_HARD_CEILING_BYTES,
        max_generation_slots: GC1_MAX_GENERATION_SLOTS,
    }
}

/// Family-level outcome under the common admission policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FamilyAdmission {
    Admitted,
    Blocked(&'static str),
}

pub const FAMILY_BLOCKED_UNKNOWN_INVENTORY: &str = "unknown-inventory";

pub const FAMILY_BLOCKED_UNKNOWN_BOUND: &str = "unknown-bound";

pub const FAMILY_BLOCKED_EXCEEDS_HARD_CEILING: &str = "exceeds-hard-ceiling";

/// Apply the common admission outcome to one family candidate.
///
/// Unknown inventory or an unknown candidate bound blocks the candidate;
/// unknowns never validate as zero. Full multi-family admission stays
/// owned by the common admission policy.
#[must_use]
pub fn admit_family_candidate(
    inventory_complete: bool,
    candidate_upper_bound_bytes: Option<u64>,
) -> FamilyAdmission {
    if !inventory_complete {
        return FamilyAdmission::Blocked(FAMILY_BLOCKED_UNKNOWN_INVENTORY);
    }
    let Some(bound) = candidate_upper_bound_bytes else {
        return FamilyAdmission::Blocked(FAMILY_BLOCKED_UNKNOWN_BOUND);
    };
    if bound > GC1_HARD_CEILING_BYTES {
        return FamilyAdmission::Blocked(FAMILY_BLOCKED_EXCEEDS_HARD_CEILING);
    }
    FamilyAdmission::Admitted
}

/// Inventory classification for one observed cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryKeyClass {
    Managed(CacheFamily),
    Unmanaged,
}

/// Classify one inventoried key without inventing a new family.
///
/// Image namespaces, old namespaces, and foreign entries classify as
/// unmanaged. They count toward inventory bytes but never authorize a
/// managed generation. Image namespaces enter accounting only when the
/// inventory actually shows them.
#[must_use]
pub fn classify_inventory_key(key: &str) -> InventoryKeyClass {
    match CacheFamily::for_key(key) {
        Some(family) => InventoryKeyClass::Managed(family),
        None => InventoryKeyClass::Unmanaged,
    }
}

impl FamilySpec {
    #[must_use]
    pub fn resolved_paths(&self, root_locator: Option<&str>) -> Vec<String> {
        self.paths
            .iter()
            .map(|path| match root_locator {
                Some(locator) => path.replace(ROOT_PLACEHOLDER, locator),
                None => (*path).to_owned(),
            })
            .collect()
    }

    #[must_use]
    pub fn path_input(&self, root_locator: Option<&str>) -> String {
        self.resolved_paths(root_locator).join("\n")
    }

    #[must_use]
    pub fn shell_paths(&self, root_locator: Option<&str>) -> Vec<String> {
        self.resolved_paths(root_locator)
            .into_iter()
            .map(|path| path.replace(RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL))
            .collect()
    }

    #[must_use]
    pub fn is_per_root(&self) -> bool {
        self.paths
            .iter()
            .any(|path| path.contains(ROOT_PLACEHOLDER))
    }
}

impl FamilySpec {
    /// Confirm this payload stays subordinate to the common ceiling.
    ///
    /// Per-family byte hints guide transport sizing only. They never
    /// authorize publication; the common admission policy owns that
    /// decision under [`GC1_HARD_CEILING_BYTES`].
    pub fn check_subordinate_to_common_ceiling(&self) -> Result<(), ContractError> {
        if self.ceiling_bytes > GC1_HARD_CEILING_BYTES {
            return Err(ContractError::identity(
                "cache_family.ceiling",
                format!("exceeds_common_ceiling:{}", self.layer),
            ));
        }
        Ok(())
    }
}

#[must_use]
pub fn spec_pin_digest(spec: &FamilySpec) -> String {
    let mut bytes = format!("layer={}\nschema={}\n", spec.layer, spec.schema);
    for path in spec.paths {
        bytes.push_str(path);
        bytes.push('\n');
    }
    blake3::hash(bytes.as_bytes()).to_hex().to_string()
}
