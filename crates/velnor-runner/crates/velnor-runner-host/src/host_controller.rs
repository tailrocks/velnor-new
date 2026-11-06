// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "host_controller/assigned_population.rs"]
mod assigned_population;
pub(crate) use assigned_population::{
    AssignedPopulationAction, plan_assigned_population_with_successors,
};
mod config_watch;
#[path = "host_controller/loop.rs"]
mod loop_driver;
mod retirement;

pub(crate) use loop_driver::connection_pat;
pub use loop_driver::run_host_blocking;
