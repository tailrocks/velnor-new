// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, finish};

#[test]
fn a_workspace_path_that_is_not_utf8_is_rejected_before_it_is_emptied() -> Result<()> {
    let rig = Rig::new()?;
    let scratch = Scratch::new()?;
    let workspace = scratch
        .workspace()
        .with_file_name(OsStr::from_bytes(b"workspace-\xff"));
    std::fs::create_dir_all(&workspace)?;
    std::fs::write(workspace.join("keep"), "precious")?;
    let job = rig.job("checkout_depth1", &rig.head, &rig.server_url())?;
    let ran = finish(rig.start_at(&job, &workspace, &scratch, &[])?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.contains("workspace is not UTF-8"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    let kept = std::fs::read_to_string(workspace.join("keep")).ok();
    assert_eq!(kept.as_deref(), Some("precious"));
    Ok(())
}
