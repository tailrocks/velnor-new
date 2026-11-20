// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::error::CiError;

use super::crates_mirror::failed;

pub(super) const ENV: &str = "VELNOR_CARGO_MBX";
pub(super) const VALUE: &str = "required";

pub(super) fn required() -> bool {
    std::env::var_os(ENV).is_some_and(|value| !value.is_empty())
        || super::crates_mirror_mode::recorded().is_ok_and(|mode| mode.is_some_and(|mode| mode.mbx))
}

pub(super) fn refused(cargo: &Path) -> CiError {
    CiError::new(format!(
        "mbx is declared but cargo on PATH isn't the mise mbx wrapper: {}",
        cargo.display()
    ))
}

pub(super) fn wrapper_missing() -> CiError {
    CiError::new("mbx is declared but mise's cargo wrapper is missing after activation")
}

pub(super) fn declared_here() -> Result<bool, CiError> {
    let directory = std::env::current_dir().map_err(|error| failed(&error))?;
    Ok(crate::mbx_declared::declared_in(&directory))
}

pub(super) fn export_line(declared: bool) -> String {
    format!("{ENV}={}\n", if declared { VALUE } else { "" })
}
