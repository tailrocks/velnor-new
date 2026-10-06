//! Clap parsing and presentation for `velnor-host`.

mod args;
mod compare;
mod daemon_run;
mod dispatch;
mod readiness_probe;
mod service;

pub use args::Cli;
pub use dispatch::run;

#[cfg(test)]
mod compare_tests;
