//! Version 17 tables for storage role charges and bootstrap records.

use crate::error::HostError;

include!("provisioning/ddl.rs");
include!("provisioning/transition_guards.rs");
include!("provisioning/migration.rs");
include!("provisioning/topology_migration.rs");
include!("provisioning/validation.rs");

#[cfg(test)]
#[path = "provisioning/transition_guard_tests.rs"]
mod transition_guard_tests;
