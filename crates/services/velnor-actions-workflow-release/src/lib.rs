//! Release-workflow rendering: jobs, config, gates, publish, and tree.
//!
//! Pure and total: the release tree renderer and workflow specs contain no `std::fs`,
//! `std::net`, or process calls. Callers supply all discovered inputs.

#![forbid(unsafe_code)]

pub mod release_checkout_gates;
pub mod release_config;
pub mod release_gates;
pub mod release_jobs;
pub mod release_permissions;
pub mod release_spec;
pub mod release_tree;
