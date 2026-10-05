//! DinD preparation, runner start, and cleanup tests.

mod cleanup;
mod fake;
mod preparation;
mod reconcile;
mod runner;

use crate::error::HostError;
use crate::journal::LaunchIdentity;
use crate::worker::{ResourceBudget, test_resource_budget};

fn identity() -> Result<LaunchIdentity, HostError> {
    LaunchIdentity::new(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        7,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "engine-test",
    )
}

fn resource_budget() -> Result<ResourceBudget, HostError> {
    test_resource_budget()
}
