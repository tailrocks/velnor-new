//! Run one shared host controller until the process stops.

#[path = "host_controller/assigned_population.rs"]
mod assigned_population;
pub(crate) use assigned_population::{
    AssignedPopulationAction, plan_assigned_population_with_successors,
};
mod config_watch;
#[path = "host_controller/loop.rs"]
mod loop_driver;
mod retirement;
#[cfg(test)]
#[path = "host_controller/retirement_tests.rs"]
mod retirement_tests;

pub(crate) use loop_driver::connection_pat;
pub use loop_driver::run_host_blocking;

#[cfg(test)]
#[path = "host_controller/tests.rs"]
mod tests;
