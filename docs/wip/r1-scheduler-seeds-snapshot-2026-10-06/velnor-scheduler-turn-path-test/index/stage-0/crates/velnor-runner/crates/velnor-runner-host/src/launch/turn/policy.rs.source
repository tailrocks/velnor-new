//! Validated limits that remain fixed for one queue session.

use std::future::Future;

use crate::worker::ResourceBudget;

use super::super::capacity::{CapacityDecision, CapacityHysteresis, GuestResourceLimits};

#[derive(Clone, Copy)]
pub(super) struct TurnPolicy {
    resource_budget: ResourceBudget,
    configured_max_jobs: u32,
}

impl TurnPolicy {
    pub(super) const fn new(resource_budget: ResourceBudget, configured_max_jobs: u32) -> Self {
        Self {
            resource_budget,
            configured_max_jobs,
        }
    }

    pub(super) fn capacity(
        self,
        policy: &mut CapacityHysteresis,
        resources: GuestResourceLimits,
        occupied: u32,
    ) -> CapacityDecision {
        policy.update(self.configured_max_jobs, resources, occupied)
    }

    pub(super) async fn scale_session<F, Fut, T>(self, launch: F) -> T
    where
        F: FnOnce(ResourceBudget) -> Fut,
        Fut: Future<Output = T>,
    {
        launch(self.resource_budget).await
    }

    pub(super) async fn drive_ready<F, Fut, T>(self, launch: F) -> T
    where
        F: FnOnce(ResourceBudget) -> Fut,
        Fut: Future<Output = T>,
    {
        launch(self.resource_budget).await
    }
}
