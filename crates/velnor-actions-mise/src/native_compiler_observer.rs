// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use crate::MiseError;
use crate::compiler_observer::{
    CompilerCompletion, OPERATION_COMPILATION, OPERATION_COMPILER_QUERY, ROLE_APPLICATION,
    ROLE_DEPENDENCY, is_supported_dependency_role, is_supported_operation_kind,
};

#[path = "native_compiler_observer_kinds.rs"]
mod native_compiler_observer_kinds;
#[path = "native_compiler_observer_ledger.rs"]
mod native_compiler_observer_ledger;
#[path = "native_compiler_observer_ops.rs"]
mod native_compiler_observer_ops;
pub use native_compiler_observer_kinds::*;
pub use native_compiler_observer_ops::*;
