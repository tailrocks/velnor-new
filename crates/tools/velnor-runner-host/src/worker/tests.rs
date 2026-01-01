//! Create projection. No live Docker daemon.

use crate::{
    BollardCreate, CreateProjection, HostError, bollard_create, runner_create, runner_plan,
};

pub(super) fn projection(volume: &str) -> Result<CreateProjection, HostError> {
    runner_create(&runner_plan(volume)?)
}

pub(super) fn bollard(volume: &str) -> Result<BollardCreate, HostError> {
    bollard_create(&projection(volume)?)
}

mod create_tests;
mod start_tests;
