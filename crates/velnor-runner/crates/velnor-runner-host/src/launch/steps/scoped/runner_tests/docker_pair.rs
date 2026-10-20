use crate::launch::resource_probe::StartPermit;
use serde_json::Value;

use super::support::{FakeLane, Fixture, REQUEST_ID, SET_ID, exchange, selected_engine};

#[path = "docker_pair/bootstrap_start.rs"]
mod bootstrap_start;
#[path = "docker_pair/native_intake_composition.rs"]
mod native_intake_composition;
#[cfg(test)]
pub(crate) use native_intake_composition::run_native_quarantine_composition;
#[path = "docker_pair/request_checks.rs"]
mod request_checks;
#[path = "docker_pair/server.rs"]
mod server;

pub(crate) use bootstrap_start::{
    RunnerCreateFailure, runner_create_409, runner_create_409_with_jobs,
};

use server::{CapturedApi, CapturedRequest, DIND_ID, PairDockerApi, RUNNER_ID, route};
