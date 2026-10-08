//! Same-scope Actions Service runner-group and Scale Set read evidence.

use std::time::SystemTime;

use super::{RunnerGroup, ScaleSetView};

/// Opaque read result joining the configured internal group to one existing
/// Scale Set through the pinned Actions Service API.
///
/// This contains no credential and is not a pool permit. It records the
/// registration scope supplied by the organization discovery bootstrap, the
/// Actions Service group ID returned in that scope, and the exact set returned
/// by a query filtered by that group ID and set name. REST group IDs are
/// intentionally absent and must never be compared with `runner_group_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsServiceScaleSetRoute {
    organization: String,
    inventory_group_count: usize,
    runner_group_id: i64,
    runner_group_name: String,
    scale_set: ScaleSetView,
    observed_at: SystemTime,
}

impl ActionsServiceScaleSetRoute {
    #[cfg(test)]
    pub(crate) fn from_test_parts(
        organization: String,
        inventory_group_count: usize,
        group: RunnerGroup,
        scale_set: ScaleSetView,
        observed_at: SystemTime,
    ) -> Self {
        Self {
            organization,
            inventory_group_count,
            runner_group_id: group.id,
            runner_group_name: group.name,
            scale_set,
            observed_at,
        }
    }

    pub(in crate::registration) fn new(
        organization: String,
        inventory_group_count: usize,
        group: RunnerGroup,
        scale_set: ScaleSetView,
    ) -> Self {
        Self {
            organization,
            inventory_group_count,
            runner_group_id: group.id,
            runner_group_name: group.name,
            scale_set,
            observed_at: SystemTime::now(),
        }
    }

    /// Organization scope of the Actions Service credential used for both
    /// internal group and Set reads.
    #[must_use]
    pub fn organization(&self) -> &str {
        &self.organization
    }

    /// Number of distinct internal groups returned by the same-scope service
    /// inventory used for unique exact-name resolution.
    #[must_use]
    pub const fn inventory_group_count(&self) -> usize {
        self.inventory_group_count
    }

    /// Actions Service internal runner-group ID. This is not the REST group ID.
    #[must_use]
    pub const fn runner_group_id(&self) -> i64 {
        self.runner_group_id
    }

    /// Exact internal group name returned by the same-scope service.
    #[must_use]
    pub fn runner_group_name(&self) -> &str {
        &self.runner_group_name
    }

    /// Exact Scale Set returned by a group-ID and name filtered GET.
    #[must_use]
    pub const fn scale_set(&self) -> &ScaleSetView {
        &self.scale_set
    }

    /// Time at which the last Actions Service read completed.
    #[must_use]
    pub const fn observed_at(&self) -> SystemTime {
        self.observed_at
    }
}
