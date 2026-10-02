//! Clap parsing and presentation for `velnor-host`.

mod args;
mod dispatch;

pub use args::Cli;
pub use dispatch::run;
