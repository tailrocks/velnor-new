// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::fd::AsFd;

use anyhow::Result;

use super::safe_fs::{enter_directory_as, open_path};
use super::tests::Scratch;

#[test]
fn entering_a_directory_reports_whether_it_was_created() -> Result<()> {
    let scratch = Scratch::new()?;
    let root = open_path(&scratch.0)?;
    let name = OsStr::new("d");
    let (_, first) = enter_directory_as(root.as_fd(), name, 0o755)?;
    let (_, second) = enter_directory_as(root.as_fd(), name, 0o755)?;
    assert_eq!((first, second), (true, false));
    Ok(())
}
