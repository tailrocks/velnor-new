//! Cargo inventory reuse + tool-drift tests.
//!
//! Declared via `#[path]` from `inventory.rs` under `cfg(test)` so the
//! inventory module keeps the file size gate.

use std::cell::Cell;

use velnor_actions_rust_core::PackageRecord;

use super::*;

mod inventory_tests;
