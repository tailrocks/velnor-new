// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use crate::MiseError;

#[path = "compiler_observer_core.rs"]
mod compiler_observer_core;
#[path = "compiler_observer_ledger.rs"]
mod compiler_observer_ledger;
pub use compiler_observer_core::*;
pub use compiler_observer_ledger::*;
