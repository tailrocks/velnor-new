//! Clap parsing and presentation for `velnor-host`.

mod args;
mod compare;
mod dispatch;
mod service;

pub use args::Cli;
pub use dispatch::run;

#[cfg(test)]
mod compare_tests;
