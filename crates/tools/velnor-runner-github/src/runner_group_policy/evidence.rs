use std::time::SystemTime;

use crate::{SessionError, Transport};

use crate::registration::AsyncDiscoveryTransport;

use super::{
    RunnerGroupPolicySnapshot, find_organization_runner_group_policy,
    find_organization_runner_group_policy_async,
};

/// Opaque result of a complete organization runner-group inventory and detail
/// read. The snapshot proves only REST-side completeness and consistency; the
/// pool verifier must still join it to a same-scope Actions Service route.
#[derive(Debug, PartialEq, Eq)]
pub struct OrganizationRunnerGroupPolicyEvidence {
    snapshot: RunnerGroupPolicySnapshot,
    observed_at: SystemTime,
}

impl OrganizationRunnerGroupPolicyEvidence {
    #[cfg(test)]
    pub(crate) fn from_test_snapshot(
        snapshot: RunnerGroupPolicySnapshot,
        observed_at: SystemTime,
    ) -> Self {
        Self {
            snapshot,
            observed_at,
        }
    }

    /// Allowlisted REST policy from the uniquely matched group.
    #[must_use]
    pub const fn policy(&self) -> &super::ActionsRunnerGroupPolicy {
        &self.snapshot.policy
    }

    /// Complete organization group inventory size used to establish unique
    /// name resolution.
    #[must_use]
    pub const fn inventory_group_count(&self) -> usize {
        self.snapshot.inventory_group_count
    }

    /// Time at which all group, policy, and selected-repository reads completed.
    #[must_use]
    pub const fn observed_at(&self) -> SystemTime {
        self.observed_at
    }
}

/// Perform the complete same-organization REST group policy read and issue an
/// opaque consistency result. `None` is an exact group-name miss, not a reason
/// to create a group or fall back to another scope.
///
/// The caller must bind `transport` to the fixed GitHub REST origin and enforce
/// its bounded, no-redirect request policy. This function performs GET-only
/// requests and never mutates runner-group configuration.
///
/// # Errors
///
/// Returns secret-safe errors for failed or incomplete inventory/detail reads.
pub fn read_organization_runner_group_policy_evidence<T>(
    transport: &mut T,
    organization: &str,
    exact_name: &str,
    actions_token: &str,
) -> Result<Option<OrganizationRunnerGroupPolicyEvidence>, SessionError>
where
    T: Transport + ?Sized,
{
    let Some(snapshot) =
        find_organization_runner_group_policy(transport, organization, exact_name, actions_token)?
    else {
        return Ok(None);
    };
    Ok(Some(OrganizationRunnerGroupPolicyEvidence {
        snapshot,
        observed_at: SystemTime::now(),
    }))
}

/// Async fixed-path reader for a complete organization group-policy snapshot.
///
/// # Errors
///
/// Returns secret-safe errors for failed or incomplete inventory/detail reads.
pub async fn read_organization_runner_group_policy_evidence_async<T>(
    transport: &mut T,
    organization: &str,
    exact_name: &str,
    actions_token: &str,
) -> Result<Option<OrganizationRunnerGroupPolicyEvidence>, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    let Some(snapshot) = find_organization_runner_group_policy_async(
        transport,
        organization,
        exact_name,
        actions_token,
    )
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(OrganizationRunnerGroupPolicyEvidence {
        snapshot,
        observed_at: SystemTime::now(),
    }))
}
