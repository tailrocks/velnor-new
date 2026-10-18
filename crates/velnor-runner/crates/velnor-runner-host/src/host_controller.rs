//! Run one shared host controller until the process stops.

mod config_watch;
#[path = "host_controller/loop.rs"]
mod loop_driver;
mod retirement;
#[cfg(test)]
#[path = "host_controller/retirement_tests.rs"]
mod retirement_tests;

pub use loop_driver::run_host_blocking;

#[cfg(test)]
#[path = "host_controller/tests.rs"]
mod tests;
